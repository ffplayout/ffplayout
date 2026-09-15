use sqlx::{
    Executor, Row, Sqlite, SqliteConnection,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{db::models::Output, utils::errors::ProcessError};

pub async fn select_outputs(pool: &SqlitePool, channel: i32) -> Result<Vec<Output>, ProcessError> {
    const QUERY: &str = "SELECT output.*, config.channel_id
        FROM config_output output
        JOIN config ON config.id = output.config_id
        WHERE config.channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_all(pool).await?;

    Ok(result)
}

pub async fn insert_output<'e, E>(
    executor: E,
    config_id: i32,
    output: &Output,
    active: bool,
) -> Result<i32, ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    const QUERY: &str = "INSERT INTO config_output (config_id, active, name, hls_variants, stream_url, stream_type, stream_format, hls_playlist_name, hls_segment_duration, hls_list_size, desktop_fullscreen, width, height, fps, video_codec, video_options, protocol_options, muxer_options, audio_codec, audio_options, audio_bitrate) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21) RETURNING id";

    let output_id = sqlx::query(QUERY)
        .bind(config_id)
        .bind(active)
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
        .bind(&output.protocol_options)
        .bind(&output.muxer_options)
        .bind(&output.audio_codec)
        .bind(&output.audio_options)
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
    protocol_options: &str,
    muxer_options: &str,
    audio_codec: Option<&str>,
    audio_options: &str,
    audio_bitrate: Option<i64>,
) -> Result<SqliteQueryResult, ProcessError> {
    let mut connection = pool.acquire().await?;
    update_output_on(
        &mut connection,
        id,
        channel_id,
        hls_variants,
        stream_url,
        stream_type,
        stream_format,
        hls_playlist_name,
        hls_segment_duration,
        hls_list_size,
        desktop_fullscreen,
        width,
        height,
        fps,
        video_codec,
        video_options,
        protocol_options,
        muxer_options,
        audio_codec,
        audio_options,
        audio_bitrate,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn update_output_on(
    connection: &mut SqliteConnection,
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
    protocol_options: &str,
    muxer_options: &str,
    audio_codec: Option<&str>,
    audio_options: &str,
    audio_bitrate: Option<i64>,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE config_output SET hls_variants = $3, stream_url = $4, stream_type = $5, stream_format = $6, hls_playlist_name = $7, hls_segment_duration = $8, hls_list_size = $9, desktop_fullscreen = $10, width = $11, height = $12, fps = $13, video_codec = $14, video_options = $15, protocol_options = $16, muxer_options = $17, audio_codec = $18, audio_options = $19, audio_bitrate = $20 WHERE id = $1 AND config_id = (SELECT id FROM config WHERE channel_id = $2)";

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
        .bind(protocol_options)
        .bind(muxer_options)
        .bind(audio_codec)
        .bind(audio_options)
        .bind(audio_bitrate)
        .execute(connection)
        .await?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::{db::handles::db_migrate, utils::config::OutputMode};

    #[tokio::test]
    async fn output_insert_select_and_update_are_scoped_to_the_channel() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let output = Output::new(1, OutputMode::Stream);
        let mut connection = pool.acquire().await.unwrap();
        let id = insert_output(&mut *connection, 1, &output, false)
            .await
            .unwrap();

        let wrong_channel = update_output_on(
            &mut connection,
            id,
            999,
            "",
            "srt://example.invalid:9000",
            Some("srt"),
            None,
            None,
            None,
            None,
            false,
            1920,
            1080,
            50.0,
            Some("libx264"),
            "{}",
            "{}",
            "{}",
            Some("aac"),
            "{}",
            Some(192),
        )
        .await
        .unwrap();
        assert_eq!(wrong_channel.rows_affected(), 0);

        let updated = update_output_on(
            &mut connection,
            id,
            1,
            "",
            "srt://example.invalid:9000",
            Some("srt"),
            None,
            None,
            None,
            None,
            false,
            1920,
            1080,
            50.0,
            Some("libx264"),
            "{}",
            "{\"latency\":\"2000000\"}",
            "{\"flush_packets\":\"1\"}",
            Some("aac"),
            "{\"aac_coder\":\"fast\"}",
            Some(192),
        )
        .await
        .unwrap();
        assert_eq!(updated.rows_affected(), 1);
        drop(connection);

        let selected = select_outputs(&pool, 1)
            .await
            .unwrap()
            .into_iter()
            .find(|output| output.id == id)
            .unwrap();
        assert_eq!(selected.channel_id, 1);
        assert_eq!(selected.stream_url, "srt://example.invalid:9000");
        assert_eq!((selected.width, selected.height), (1920, 1080));
        assert_eq!(selected.fps, 50.0);
        assert_eq!(selected.protocol_options, "{\"latency\":\"2000000\"}");
        assert_eq!(selected.muxer_options, "{\"flush_packets\":\"1\"}");
        assert_eq!(selected.audio_options, "{\"aac_coder\":\"fast\"}");
    }
}
