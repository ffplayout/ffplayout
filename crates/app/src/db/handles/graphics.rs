use sqlx::{
    SqlitePool,
    migrate::{Migration, MigrationType, Migrator},
};

use crate::{db::models::GraphicLayer, utils::errors::ProcessError};

const LEGACY_MIGRATION: &str = "ALTER TABLE config_live_input ADD COLUMN auto_takeover INTEGER NOT NULL DEFAULT 1\n    CHECK (auto_takeover IN (0, 1));\n";

/// Only the exact previously shipped development migration is eligible for upgrade.
/// Unknown checksums still go through SQLx's normal mismatch rejection.
pub(super) async fn upgrade_manual_graphics_schema(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> Result<(), ProcessError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;

    if !exists {
        return Ok(());
    }

    let old = Migration::new(
        8,
        "manual live takeover".into(),
        MigrationType::Simple,
        sqlx::SqlStr::from_static(LEGACY_MIGRATION),
        false,
    );
    let Some(current) = migrator.iter().find(|migration| migration.version == 8) else {
        return Ok(());
    };
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations")
            .fetch_all(pool)
            .await?;
    let legacy = applied.iter().any(|(version, checksum, success)| {
        *version == 8 && *success && checksum.as_slice() == old.checksum.as_ref()
    });
    let compatible = applied.iter().all(|(version, checksum, success)| {
        *success
            && migrator.iter().any(|migration| {
                migration.version == *version
                    && (checksum.as_slice() == migration.checksum.as_ref()
                        || (*version == 8 && checksum.as_slice() == old.checksum.as_ref()))
            })
    });

    if !legacy || !compatible {
        return Ok(());
    }

    let schema = include_str!("../../../../../migrations/00008_live_takeover_and_graphics.sql")
        .split_once("-- Persistent manual program graphics.\n")
        .ok_or_else(|| ProcessError::DB("Graphics migration extension missing".into()))?
        .1;
    let mut transaction = pool.begin().await?;
    sqlx::raw_sql(schema).execute(&mut *transaction).await?;
    sqlx::query("UPDATE _sqlx_migrations SET description = $1, checksum = $2 WHERE version = 8 AND checksum = $3")
        .bind(current.description.as_ref())
        .bind(current.checksum.as_ref())
        .bind(old.checksum.as_ref())
        .execute(&mut *transaction).await?;
    transaction.commit().await?;

    Ok(())
}

pub async fn select_graphic_layers(
    pool: &SqlitePool,
    channel: i32,
) -> Result<(i64, Vec<GraphicLayer>), ProcessError> {
    let mut transaction = pool.begin().await?;
    let revision =
        sqlx::query_scalar("SELECT revision FROM config_graphics_state WHERE channel_id = $1")
            .bind(channel)
            .fetch_optional(&mut *transaction)
            .await?
            .unwrap_or(0);
    let layers = sqlx::query_as("SELECT layer_id, name, settings, running, visible FROM config_graphics WHERE channel_id = $1 ORDER BY layer_id")
        .bind(channel).fetch_all(&mut *transaction).await?;
    transaction.commit().await?;

    Ok((revision, layers))
}

/// Commit a bounded desired-state snapshot and its revision together.
pub async fn store_graphic_layers(
    pool: &SqlitePool,
    channel: i32,
    expected: i64,
    layers: &[GraphicLayer],
) -> Result<i64, ProcessError> {
    if layers.len() > 8 || expected == i64::MAX {
        return Err(ProcessError::Custom(
            "Graphics state exceeds its limits".into(),
        ));
    }

    let mut transaction = pool.begin().await?;
    sqlx::query("INSERT INTO config_graphics_state(channel_id) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(channel)
        .execute(&mut *transaction)
        .await?;
    let changed = sqlx::query("UPDATE config_graphics_state SET revision = revision + 1 WHERE channel_id = $1 AND revision = $2")
        .bind(channel).bind(expected).execute(&mut *transaction).await?;

    if changed.rows_affected() != 1 {
        return Err(ProcessError::Custom(
            "Graphics revision changed; refresh before retrying".into(),
        ));
    }

    sqlx::query("DELETE FROM config_graphics WHERE channel_id = $1")
        .bind(channel)
        .execute(&mut *transaction)
        .await?;

    for layer in layers {
        sqlx::query("INSERT INTO config_graphics(channel_id, layer_id, name, settings, running, visible) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(channel).bind(layer.layer_id).bind(&layer.name).bind(&layer.settings).bind(layer.running).bind(layer.visible)
            .execute(&mut *transaction).await?;
    }

    transaction.commit().await?;

    Ok(expected + 1)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use serde_json::json;
    use sqlx::{SqlStr, sqlite::SqlitePoolOptions, types::Json};

    use crate::db::handles::db_migrate;

    use super::*;

    async fn legacy_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let mut migrator = sqlx::migrate!("../../migrations");
        migrator.migrations = Cow::Owned(
            migrator
                .iter()
                .filter(|migration| migration.version < 8)
                .cloned()
                .collect(),
        );
        migrator.run(&pool).await.unwrap();
        sqlx::raw_sql(LEGACY_MIGRATION)
            .execute(&pool)
            .await
            .unwrap();
        let old = Migration::new(
            8,
            "manual live takeover".into(),
            MigrationType::Simple,
            SqlStr::from_static(LEGACY_MIGRATION),
            false,
        );
        sqlx::query("INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) VALUES(8, 'manual live takeover', 1, $1, 0)")
            .bind(old.checksum.as_ref()).execute(&pool).await.unwrap();

        pool
    }

    #[tokio::test]
    async fn known_legacy_migration_upgrades_without_repeating_takeover_and_is_idempotent() {
        let pool = legacy_pool().await;
        sqlx::query("UPDATE config_live_input SET auto_takeover = 0")
            .execute(&pool)
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let settings = Json(json!({"action":"take","at":0,"layer":1,"source":"logo.png"}));
        let row = GraphicLayer {
            layer_id: 1,
            name: "Logo".into(),
            settings,
            running: true,
            visible: false,
        };
        assert_eq!(store_graphic_layers(&pool, 1, 0, &[row]).await.unwrap(), 1);
        db_migrate(&pool).await.unwrap();
        let (revision, layers) = select_graphic_layers(&pool, 1).await.unwrap();
        assert_eq!(revision, 1);
        assert_eq!(layers[0].name, "Logo");
        assert!(layers[0].running && !layers[0].visible);
        let enabled: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM config_live_input WHERE auto_takeover != 0")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(enabled, 0);
        let checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 8")
                .fetch_one(&pool)
                .await
                .unwrap();
        let migrator = sqlx::migrate!("../../migrations");
        assert_eq!(
            checksum,
            migrator
                .iter()
                .find(|migration| migration.version == 8)
                .unwrap()
                .checksum
                .as_ref()
        );
    }

    #[tokio::test]
    async fn unknown_migration_checksum_is_rejected_without_creating_graphics_tables() {
        for version in [7, 8] {
            let pool = legacy_pool().await;
            sqlx::query("UPDATE _sqlx_migrations SET checksum = X'0102' WHERE version = $1")
                .bind(version)
                .execute(&pool)
                .await
                .unwrap();
            assert!(db_migrate(&pool).await.is_err());
            let tables: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'config_graphics'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(tables, 0);
        }
    }

    #[tokio::test]
    async fn saved_layers_are_channel_scoped_revision_checked_and_transactional() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        sqlx::query("INSERT INTO channels(id, name, preview_url) VALUES(2, 'Second', '')")
            .execute(&pool)
            .await
            .unwrap();
        let row = GraphicLayer {
            layer_id: 1,
            name: "First".into(),
            settings: Json(json!({"source":"logo.png"})),
            running: false,
            visible: true,
        };
        store_graphic_layers(&pool, 1, 0, std::slice::from_ref(&row))
            .await
            .unwrap();
        assert!(store_graphic_layers(&pool, 1, 0, &[]).await.is_err());
        let (_, saved) = select_graphic_layers(&pool, 1).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(select_graphic_layers(&pool, 2).await.unwrap().0, 0);
        assert!(select_graphic_layers(&pool, 2).await.unwrap().1.is_empty());
        let mut invalid = row;
        invalid.layer_id = -1;
        assert!(store_graphic_layers(&pool, 1, 1, &[invalid]).await.is_err());
        let (revision, saved) = select_graphic_layers(&pool, 1).await.unwrap();
        assert_eq!(revision, 1);
        assert_eq!(saved[0].name, "First");
        sqlx::query("DELETE FROM channels WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(select_graphic_layers(&pool, 1).await.unwrap().1.is_empty());
    }
}
