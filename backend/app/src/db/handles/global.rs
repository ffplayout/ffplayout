use sqlx::sqlite::{SqlitePool, SqliteQueryResult};

use crate::{db::GlobalSettings, utils::errors::ProcessError};

pub async fn select_global(pool: &SqlitePool) -> Result<GlobalSettings, ProcessError> {
    const QUERY: &str = "SELECT id, secret, logs, playlists, public, storage, shared, smtp_server, smtp_user, smtp_password, smtp_starttls, smtp_port FROM global WHERE id = 1";

    let result = sqlx::query_as(QUERY).fetch_one(pool).await?;

    Ok(result)
}

pub async fn update_global(
    pool: &SqlitePool,
    global: GlobalSettings,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE global SET logs = $2, playlists = $3, public = $4, storage = $5,
            smtp_server = $6, smtp_user = $7, smtp_password = $8, smtp_starttls = $9, smtp_port = $10  WHERE id = 1";

    let result = sqlx::query(QUERY)
        .bind(global.id)
        .bind(global.logs)
        .bind(global.playlists)
        .bind(global.public)
        .bind(global.storage)
        .bind(global.smtp_server)
        .bind(global.smtp_user)
        .bind(global.smtp_password)
        .bind(global.smtp_starttls)
        .bind(global.smtp_port)
        .execute(pool)
        .await?;

    Ok(result)
}
