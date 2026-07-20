use sqlx::{
    Executor, Row, Sqlite,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{db::models::Output, utils::errors::ProcessError};

pub async fn select_outputs(pool: &SqlitePool, channel: i32) -> Result<Vec<Output>, ProcessError> {
    const QUERY: &str = "SELECT * FROM outputs WHERE channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_output<'e, E>(
    executor: E,
    channel_id: i32,
    output: &Output,
) -> Result<i32, ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    const QUERY: &str = "INSERT INTO outputs (channel_id, name, hls_variants, stream_url, stream_type, stream_format, hls_playlist_name, hls_segment_duration, hls_list_size, desktop_fullscreen, width, height, fps, video_codec, video_options, audio_codec, audio_bitrate) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) RETURNING id";

    let output_id = sqlx::query(QUERY)
        .bind(channel_id)
        .bind(&output.name)
        .bind(&output.hls_variants)
        .bind(&output.stream_url)
        .bind(&output.stream_type)
        .bind(&output.stream_format)
        .bind(&output.hls_playlist_name)
        .bind(output.hls_segment_duration)
        .bind(output.hls_list_size)
        .bind(output.desktop_fullscreen)
        .bind(output.width)
        .bind(output.height)
        .bind(output.fps)
        .bind(&output.video_codec)
        .bind(&output.video_options)
        .bind(&output.audio_codec)
        .bind(output.audio_bitrate)
        .fetch_one(executor)
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
    stream_type: Option<&str>,
    stream_format: Option<&str>,
    hls_playlist_name: Option<&str>,
    hls_segment_duration: Option<i64>,
    hls_list_size: Option<i64>,
    desktop_fullscreen: bool,
    width: i64,
    height: i64,
    fps: f64,
    video_codec: Option<&str>,
    video_options: &str,
    audio_codec: Option<&str>,
    audio_bitrate: Option<i64>,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE outputs SET hls_variants = $3, stream_url = $4, stream_type = $5, stream_format = $6, hls_playlist_name = $7, hls_segment_duration = $8, hls_list_size = $9, desktop_fullscreen = $10, width = $11, height = $12, fps = $13, video_codec = $14, video_options = $15, audio_codec = $16, audio_bitrate = $17 WHERE id = $1 AND channel_id = $2";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel_id)
        .bind(hls_variants)
        .bind(stream_url)
        .bind(stream_type)
        .bind(stream_format)
        .bind(hls_playlist_name)
        .bind(hls_segment_duration)
        .bind(hls_list_size)
        .bind(desktop_fullscreen)
        .bind(width)
        .bind(height)
        .bind(fps)
        .bind(video_codec)
        .bind(video_options)
        .bind(audio_codec)
        .bind(audio_bitrate)
        .execute(pool)
        .await?;

    Ok(result)
}
