use sqlx::sqlite::{SqlitePool, SqliteQueryResult};

use crate::{db::models::Channel, utils::errors::ProcessError};

pub async fn select_channel(pool: &SqlitePool, id: &i32) -> Result<Channel, ProcessError> {
    const QUERY: &str = "SELECT * FROM channels WHERE id = $1";

    let result = sqlx::query_as(QUERY).bind(id).fetch_one(pool).await?;

    Ok(result)
}

pub async fn select_related_channels(
    pool: &SqlitePool,
    user_id: Option<i32>,
) -> Result<Vec<Channel>, ProcessError> {
    let query = match user_id {
        Some(id) => format!(
            "SELECT c.id, c.name, c.preview_url, c.extra_extensions, c.active, c.public, c.playlists,
            c.storage, c.last_date, c.time_shift, c.timezone, c.advanced_id FROM channels c
                left join user_channels uc on uc.channel_id = c.id
                left join user u on u.id = uc.user_id
             WHERE u.id = {id} ORDER BY c.id ASC;"
        ),
        None => "SELECT * FROM channels ORDER BY id ASC;".to_string(),
    };

    let result = sqlx::query_as(&query).fetch_all(pool).await?;

    Ok(result)
}

pub async fn delete_user_channel(
    pool: &SqlitePool,
    user_id: i32,
    channel_id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM user_channels WHERE user_id = $1 AND channel_id = $2";

    let result = sqlx::query(QUERY)
        .bind(user_id)
        .bind(channel_id)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn update_channel(
    pool: &SqlitePool,
    id: i32,
    channel: Channel,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE channels SET name = $2, preview_url = $3, extra_extensions = $4, public = $5, playlists = $6, storage = $7, timezone = $8 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.public)
        .bind(channel.playlists)
        .bind(channel.storage)
        .bind(channel.timezone.map(|tz| tz.to_string()))
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn insert_channel(pool: &SqlitePool, channel: Channel) -> Result<Channel, ProcessError> {
    const QUERY: &str = "INSERT INTO channels (name, preview_url, extra_extensions, public, playlists, storage) VALUES($1, $2, $3, $4, $5, $6)";
    let result = sqlx::query(QUERY)
        .bind(channel.name)
        .bind(channel.preview_url)
        .bind(channel.extra_extensions)
        .bind(channel.public)
        .bind(channel.playlists)
        .bind(channel.storage)
        .execute(pool)
        .await?;

    let result = sqlx::query_as("SELECT * FROM channels WHERE id = $1")
        .bind(result.last_insert_rowid())
        .fetch_one(pool)
        .await?;

    Ok(result)
}

pub async fn delete_channel(
    pool: &SqlitePool,
    id: &i32,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "DELETE FROM channels WHERE id = $1";

    let result = sqlx::query(QUERY).bind(id).execute(pool).await?;

    Ok(result)
}

pub async fn select_last_channel(pool: &SqlitePool) -> Result<i32, ProcessError> {
    const QUERY: &str = "select seq from sqlite_sequence WHERE name = 'channel';";

    let result = sqlx::query_scalar(QUERY).fetch_one(pool).await?;

    Ok(result)
}

pub async fn update_stat(
    pool: &SqlitePool,
    id: i32,
    last_date: &Option<String>,
    time_shift: f64,
) -> Result<SqliteQueryResult, ProcessError> {
    let query = match last_date {
        Some(_) => "UPDATE channels SET last_date = $2, time_shift = $3 WHERE id = $1",
        None => "UPDATE channels SET time_shift = $2 WHERE id = $1",
    };

    let mut q = sqlx::query(query).bind(id);

    if last_date.is_some() {
        q = q.bind(last_date);
    }

    let result = q.bind(time_shift).execute(pool).await?;

    Ok(result)
}

pub async fn update_player(
    pool: &SqlitePool,
    id: i32,
    active: bool,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE channels SET active = $2 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(active)
        .execute(pool)
        .await?;

    Ok(result)
}
