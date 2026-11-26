use std::collections::BTreeMap;

use sqlx::{QueryBuilder, SqliteConnection, SqlitePool, types::Json};

use crate::{
    db::models::{GraphicAppointment, GraphicLayer},
    utils::errors::ProcessError,
};

pub type GraphicSchedules = BTreeMap<i64, Vec<GraphicAppointment>>;

pub async fn select_graphic_layers(
    pool: &SqlitePool,
    channel: i32,
) -> Result<(i64, Vec<GraphicLayer>), ProcessError> {
    let mut transaction = pool.begin().await?;
    let snapshot = select_layers(&mut transaction, channel).await?;

    transaction.commit().await?;

    Ok(snapshot)
}

async fn select_layers(
    connection: &mut SqliteConnection,
    channel: i32,
) -> Result<(i64, Vec<GraphicLayer>), ProcessError> {
    let revision =
        sqlx::query_scalar("SELECT revision FROM config_graphics_state WHERE channel_id = $1")
            .bind(channel)
            .fetch_optional(&mut *connection)
            .await?
            .unwrap_or(0);
    let layers = sqlx::query_as("SELECT layer_id, name, settings, running, visible FROM config_graphics WHERE channel_id = $1 ORDER BY layer_id")
        .bind(channel).fetch_all(connection).await?;

    Ok((revision, layers))
}

pub async fn select_graphics(
    pool: &SqlitePool,
    channel: i32,
) -> Result<(i64, Vec<GraphicLayer>, GraphicSchedules), ProcessError> {
    let mut transaction = pool.begin().await?;
    let (revision, layers) = select_layers(&mut transaction, channel).await?;
    let rows: Vec<(i64, Json<GraphicAppointment>)> = sqlx::query_as(
        "SELECT layer_id, appointment FROM config_graphics_schedule WHERE channel_id = $1 ORDER BY layer_id, position",
    )
    .bind(channel)
    .fetch_all(&mut *transaction)
    .await?;
    let mut schedules = GraphicSchedules::new();

    for (layer, Json(appointment)) in rows {
        schedules.entry(layer).or_default().push(appointment);
    }

    transaction.commit().await?;

    Ok((revision, layers, schedules))
}

async fn bump_revision(
    connection: &mut SqliteConnection,
    channel: i32,
    expected: i64,
) -> Result<i64, ProcessError> {
    if expected == i64::MAX {
        return Err(ProcessError::Custom(
            "Graphics revision exceeds its limit".into(),
        ));
    }

    sqlx::query("INSERT INTO config_graphics_state(channel_id) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(channel)
        .execute(&mut *connection)
        .await?;
    let changed = sqlx::query("UPDATE config_graphics_state SET revision = revision + 1 WHERE channel_id = $1 AND revision = $2")
        .bind(channel).bind(expected).execute(connection).await?;

    if changed.rows_affected() != 1 {
        return Err(ProcessError::Custom(
            "Graphics revision changed; refresh before retrying".into(),
        ));
    }

    Ok(expected + 1)
}

pub async fn store_graphic_schedule(
    pool: &SqlitePool,
    channel: i32,
    expected: i64,
    layer: i64,
    appointments: &[GraphicAppointment],
    baseline: Option<&[GraphicAppointment]>,
) -> Result<i64, ProcessError> {
    let mut transaction = pool.begin().await?;
    let revision = bump_revision(&mut transaction, channel, expected).await?;

    if let Some(baseline) = baseline {
        let saved: Vec<Json<GraphicAppointment>> = sqlx::query_scalar(
            "SELECT appointment FROM config_graphics_schedule WHERE channel_id = $1 AND layer_id = $2 ORDER BY position",
        )
        .bind(channel)
        .bind(layer)
        .fetch_all(&mut *transaction)
        .await?;

        if saved.iter().map(|appointment| &appointment.0).ne(baseline) {
            return Err(ProcessError::Custom(
                "Graphics schedule changed; reload before retrying".into(),
            ));
        }
    }

    sqlx::query("DELETE FROM config_graphics_schedule WHERE channel_id = $1 AND layer_id = $2")
        .bind(channel)
        .bind(layer)
        .execute(&mut *transaction)
        .await?;

    for (position, appointment) in appointments.iter().enumerate() {
        sqlx::query("INSERT INTO config_graphics_schedule(channel_id, layer_id, position, appointment) VALUES($1,$2,$3,$4)")
            .bind(channel).bind(layer).bind(position as i64).bind(Json(appointment))
            .execute(&mut *transaction).await?;
    }

    transaction.commit().await?;

    Ok(revision)
}

/// Commit a bounded desired-state snapshot and its revision together.
pub async fn store_graphic_layers(
    pool: &SqlitePool,
    channel: i32,
    expected: i64,
    layers: &[GraphicLayer],
) -> Result<i64, ProcessError> {
    if layers.len() > 8 {
        return Err(ProcessError::Custom(
            "Graphics state exceeds its limits".into(),
        ));
    }

    let mut transaction = pool.begin().await?;
    let revision = bump_revision(&mut transaction, channel, expected).await?;
    let mut delete = QueryBuilder::new("DELETE FROM config_graphics WHERE channel_id = ");

    delete.push_bind(channel);

    if !layers.is_empty() {
        delete.push(" AND layer_id NOT IN (");
        let mut ids = delete.separated(", ");

        for layer in layers {
            ids.push_bind(layer.layer_id);
        }

        ids.push_unseparated(")");
    }

    delete.build().execute(&mut *transaction).await?;

    for layer in layers {
        sqlx::query("INSERT INTO config_graphics(channel_id, layer_id, name, settings, running, visible) VALUES($1,$2,$3,$4,$5,$6)
            ON CONFLICT(channel_id, layer_id) DO UPDATE SET name=excluded.name, settings=excluded.settings, running=excluded.running, visible=excluded.visible")
            .bind(channel).bind(layer.layer_id).bind(&layer.name).bind(&layer.settings).bind(layer.running).bind(layer.visible)
            .execute(&mut *transaction).await?;
    }

    transaction.commit().await?;

    Ok(revision)
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
