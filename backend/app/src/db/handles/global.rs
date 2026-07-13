use sqlx::sqlite::{SqlitePool, SqliteQueryResult};

use crate::{db::GlobalSettings, utils::errors::ProcessError};

pub async fn select_global(pool: &SqlitePool) -> Result<GlobalSettings, ProcessError> {
    const QUERY: &str = "SELECT id, secret, logs, playlists, public, storage, shared, smtp_server, smtp_user, smtp_password, smtp_starttls, smtp_port, setup_completed FROM global WHERE id = 1";

    let result = sqlx::query_as(QUERY).fetch_one(pool).await?;

    Ok(result)
}

pub async fn update_global(
    pool: &SqlitePool,
    global: GlobalSettings,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE global SET logs = $2, playlists = $3, public = $4, storage = $5, shared = $6,
            smtp_server = $7, smtp_user = $8, smtp_password = $9, smtp_starttls = $10, smtp_port = $11 WHERE id = 1";

    let result = sqlx::query(QUERY)
        .bind(global.id)
        .bind(global.logs)
        .bind(global.playlists)
        .bind(global.public)
        .bind(global.storage)
        .bind(global.shared)
        .bind(global.smtp_server)
        .bind(global.smtp_user)
        .bind(global.smtp_password)
        .bind(global.smtp_starttls)
        .bind(global.smtp_port)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn mark_setup_completed(pool: &SqlitePool) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE global SET setup_completed = 1 WHERE id = 1";

    Ok(sqlx::query(QUERY).execute(pool).await?)
}
