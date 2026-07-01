use sqlx::{
    Row,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{db::models::Output, utils::errors::ProcessError};

pub async fn select_outputs(pool: &SqlitePool, channel: i32) -> Result<Vec<Output>, ProcessError> {
    const QUERY: &str = "SELECT * FROM outputs WHERE channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_output(
    pool: &SqlitePool,
    channel_id: i32,
    output: &Output,
) -> Result<i32, ProcessError> {
    const QUERY: &str = "INSERT INTO outputs (channel_id, name, parameters, hls_variants, stream_url, hls_playlist_path, hls_segment_duration, hls_list_size) VALUES($1, $2, '', $3, $4, $5, $6, $7) RETURNING id";

    let output_id = sqlx::query(QUERY)
        .bind(channel_id)
        .bind(&output.name)
        .bind(&output.hls_variants)
        .bind(&output.stream_url)
        .bind(&output.hls_playlist_path)
        .bind(output.hls_segment_duration)
        .bind(output.hls_list_size)
        .fetch_one(pool)
        .await?
        .get("id");

    Ok(output_id)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_output(
    pool: &SqlitePool,
    id: i32,
    channel_id: i32,
    hls_variants: &str,
    stream_url: &str,
    hls_playlist_path: Option<&str>,
    hls_segment_duration: Option<i64>,
    hls_list_size: Option<i64>,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE outputs SET hls_variants = $3, stream_url = $4, hls_playlist_path = $5, hls_segment_duration = $6, hls_list_size = $7 WHERE id = $1 AND channel_id = $2";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel_id)
        .bind(hls_variants)
        .bind(stream_url)
        .bind(hls_playlist_path)
        .bind(hls_segment_duration)
        .bind(hls_list_size)
        .execute(pool)
        .await?;

    Ok(result)
}
