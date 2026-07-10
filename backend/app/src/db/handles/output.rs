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
    const QUERY: &str = "INSERT INTO outputs (channel_id, name, hls_variants, stream_url, stream_type, hls_playlist_name, hls_segment_duration, hls_list_size, desktop_fullscreen, width, height, fps, video_preset, video_codec, audio_codec, rate_control, video_quality, video_maxrate, audio_bitrate) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19) RETURNING id";

    let output_id = sqlx::query(QUERY)
        .bind(channel_id)
        .bind(&output.name)
        .bind(&output.hls_variants)
        .bind(&output.stream_url)
        .bind(&output.stream_type)
        .bind(&output.hls_playlist_name)
        .bind(output.hls_segment_duration)
        .bind(output.hls_list_size)
        .bind(output.desktop_fullscreen)
        .bind(output.width)
        .bind(output.height)
        .bind(output.fps)
        .bind(&output.video_preset)
        .bind(&output.video_codec)
        .bind(&output.audio_codec)
        .bind(&output.rate_control)
        .bind(output.video_quality)
        .bind(output.video_maxrate)
        .bind(output.audio_bitrate)
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
    stream_type: Option<&str>,
    hls_playlist_name: Option<&str>,
    hls_segment_duration: Option<i64>,
    hls_list_size: Option<i64>,
    desktop_fullscreen: bool,
    width: i64,
    height: i64,
    fps: f64,
    video_preset: Option<&str>,
    video_codec: Option<&str>,
    audio_codec: Option<&str>,
    rate_control: Option<&str>,
    video_quality: Option<i64>,
    video_maxrate: Option<i64>,
    audio_bitrate: Option<i64>,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE outputs SET hls_variants = $3, stream_url = $4, stream_type = $5, hls_playlist_name = $6, hls_segment_duration = $7, hls_list_size = $8, desktop_fullscreen = $9, width = $10, height = $11, fps = $12, video_preset = $13, video_codec = $14, audio_codec = $15, rate_control = $16, video_quality = $17, video_maxrate = $18, audio_bitrate = $19 WHERE id = $1 AND channel_id = $2";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(channel_id)
        .bind(hls_variants)
        .bind(stream_url)
        .bind(stream_type)
        .bind(hls_playlist_name)
        .bind(hls_segment_duration)
        .bind(hls_list_size)
        .bind(desktop_fullscreen)
        .bind(width)
        .bind(height)
        .bind(fps)
        .bind(video_preset)
        .bind(video_codec)
        .bind(audio_codec)
        .bind(rate_control)
        .bind(video_quality)
        .bind(video_maxrate)
        .bind(audio_bitrate)
        .execute(pool)
        .await?;

    Ok(result)
}
