use sqlx::SqlitePool;

use crate::{db::models::GraphicLayer, utils::errors::ProcessError};

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
    use serde_json::json;
    use sqlx::{sqlite::SqlitePoolOptions, types::Json};

    use crate::db::handles::db_migrate;

    use super::*;

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
