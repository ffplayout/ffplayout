use sqlx::{
    Executor, Row, Sqlite, SqliteConnection,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{db::models::Output, utils::errors::ProcessError};

pub async fn select_outputs(pool: &SqlitePool, channel: i32) -> Result<Vec<Output>, ProcessError> {
    const QUERY: &str = "SELECT output.*, config.channel_id
        FROM config_output output
        JOIN config ON config.id = output.config_id
        WHERE config.channel_id = $1 ORDER BY output.priority, output.id";

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
    const QUERY: &str = "INSERT INTO config_output (config_id, active, name, hls_variants, stream_url, stream_type, stream_format, hls_playlist_name, hls_segment_duration, hls_list_size, desktop_fullscreen, width, height, fps, video_codec, video_options, protocol_options, muxer_options, metadata_options, audio_codec, audio_options, audio_bitrate) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22) RETURNING id";

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
        .bind(&output.metadata_options)
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
    metadata_options: &str,
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
        metadata_options,
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
    metadata_options: &str,
    audio_codec: Option<&str>,
    audio_options: &str,
    audio_bitrate: Option<i64>,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE config_output SET hls_variants = $3, stream_url = $4, stream_type = $5, stream_format = $6, hls_playlist_name = $7, hls_segment_duration = $8, hls_list_size = $9, desktop_fullscreen = $10, width = $11, height = $12, fps = $13, video_codec = $14, video_options = $15, protocol_options = $16, muxer_options = $17, metadata_options = $18, audio_codec = $19, audio_options = $20, audio_bitrate = $21 WHERE id = $1 AND config_id = (SELECT id FROM config WHERE channel_id = $2)";

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
        .bind(metadata_options)
        .bind(audio_codec)
        .bind(audio_options)
        .bind(audio_bitrate)
        .execute(connection)
        .await?;

    Ok(result)
}

/// Persist the enabled collection atomically; old disabled presets remain available.
pub async fn save_outputs_on(
    connection: &mut SqliteConnection,
    config_id: i32,
    config: &mut crate::utils::config::PlayoutConfig,
) -> Result<(), ProcessError> {
    sqlx::query("UPDATE config_output SET active = 0, configured = 0 WHERE config_id = $1")
        .bind(config_id)
        .execute(&mut *connection)
        .await?;
    let channel_id: i32 = sqlx::query_scalar("SELECT channel_id FROM config WHERE id = $1")
        .bind(config_id)
        .fetch_one(&mut *connection)
        .await?;

    for output in std::iter::once(&mut config.output).chain(config.outputs.iter_mut()) {
        output.share_encoding = output.encoding_source_id.is_some();
        let record = Output {
            id: output.id,
            channel_id,
            active: output.enabled,
            label: output.label.clone(),
            configured: true,
            inherit_resolution: output.inherit_resolution,
            share_encoding: output.share_encoding,
            encoding_source_id: output.encoding_source_id,
            encoding_variant: output.encoding_variant.clone(),
            public_moq: output.public_moq,
            runtime_status: None,
            runtime_error: None,
            runtime_retryable: false,
            name: output.mode.to_string(),
            hls_variants: output.hls_variants.join(";"),
            stream_url: output.stream_url.clone(),
            stream_type: Some(
                serde_json::to_value(output.stream_type)?
                    .as_str()
                    .unwrap_or("rtmp")
                    .to_string(),
            ),
            stream_format: Some(output.stream_format.clone()),
            hls_playlist_name: Some(output.hls_playlist_name.clone()),
            hls_segment_duration: Some(i64::from(output.hls_segment_duration)),
            hls_list_size: Some(i64::from(output.hls_list_size)),
            desktop_fullscreen: output.desktop_fullscreen,
            width: i64::from(output.width),
            height: i64::from(output.height),
            fps: output.fps,
            video_codec: Some(output.video_codec.clone()),
            video_options: serde_json::to_string(&output.video_options)?,
            protocol_options: serde_json::to_string(&output.protocol_options)?,
            muxer_options: serde_json::to_string(&output.muxer_options)?,
            metadata_options: serde_json::to_string(&output.metadata_options)?,
            audio_codec: Some(output.audio_codec.clone()),
            audio_options: serde_json::to_string(&output.audio_options)?,
            audio_bitrate: Some(i64::from(output.audio_bitrate)),
        };

        if output.id == 0 {
            output.id = insert_output(&mut *connection, config_id, &record, output.enabled).await?;
        } else {
            let result = update_output_on(
                connection,
                output.id,
                channel_id,
                &record.hls_variants,
                &record.stream_url,
                record.stream_type.as_deref(),
                record.stream_format.as_deref(),
                record.hls_playlist_name.as_deref(),
                record.hls_segment_duration,
                record.hls_list_size,
                record.desktop_fullscreen,
                record.width,
                record.height,
                record.fps,
                record.video_codec.as_deref(),
                &record.video_options,
                &record.protocol_options,
                &record.muxer_options,
                &record.metadata_options,
                record.audio_codec.as_deref(),
                &record.audio_options,
                record.audio_bitrate,
            )
            .await?;
            if result.rows_affected() != 1 {
                return Err(ProcessError::Custom(format!(
                    "Output {} does not belong to channel {channel_id}",
                    output.id
                )));
            }
        }
        sqlx::query(
            "UPDATE config_output SET active = $4, name = $3, configured = 1, label = $5, inherit_resolution = $6, share_encoding = $7, encoding_source_id = $8, encoding_variant = $9, public_moq = $10 WHERE id = $1 AND config_id = $2",
        )
        .bind(output.id)
        .bind(config_id)
        .bind(&record.name)
        .bind(output.enabled)
        .bind(&output.label)
        .bind(output.inherit_resolution)
        .bind(output.share_encoding)
        .bind(output.encoding_source_id)
        .bind(&output.encoding_variant)
        .bind(output.public_moq)
        .execute(&mut *connection)
        .await?;
    }

    sqlx::query("UPDATE config SET program_format = $2 WHERE id = $1")
        .bind(config_id)
        .bind(
            config
                .program
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        )
        .execute(&mut *connection)
        .await?;

    Ok(())
}

pub async fn select_program_format(
    pool: &SqlitePool,
    config_id: i32,
) -> Result<Option<crate::utils::config::ProgramFormat>, ProcessError> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT program_format FROM config WHERE id = $1")
            .bind(config_id)
            .fetch_one(pool)
            .await?;

    Ok(value
        .map(|value| serde_json::from_str(&value))
        .transpose()?)
}

/// Public access follows the saved output setting, independently of encoder restarts.
pub async fn moq_access(pool: &SqlitePool, channel: i32) -> Result<Option<bool>, ProcessError> {
    Ok(sqlx::query_scalar("SELECT public_moq FROM config_output WHERE config_id = (SELECT id FROM config WHERE channel_id = $1) AND active = 1 AND name = 'moq' LIMIT 1")
        .bind(channel).fetch_optional(pool).await?)
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::{db::handles::db_migrate, utils::config::OutputMode};

    #[tokio::test]
    async fn all_destinations_can_be_disabled_without_losing_configuration() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let primary = select_outputs(&pool, 1)
            .await
            .unwrap()
            .into_iter()
            .find(|output| output.active)
            .unwrap();
        let id = primary.id;
        let mut config = crate::utils::config::PlayoutConfig {
            output: crate::utils::config::Output::from_record(primary),
            ..Default::default()
        };
        config.output.enabled = false;
        assert!(config.validate_outputs().is_ok());
        let mut transaction = pool.begin().await.unwrap();
        save_outputs_on(&mut transaction, 1, &mut config)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            crate::db::handles::select_configuration(&pool, 1)
                .await
                .unwrap()
                .output_id,
            id
        );
        let stored = select_outputs(&pool, 1)
            .await
            .unwrap()
            .into_iter()
            .find(|output| output.id == id)
            .unwrap();
        assert!(stored.configured);
        assert!(!crate::utils::config::Output::from_record(stored).enabled);
    }

    #[tokio::test]
    async fn moq_uses_output_rows_and_the_schema_accepts_future_codec_names() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let side_tables: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('config_preview', 'config_moq')")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(side_tables, 0);
        let mut output = Output::new(1, OutputMode::Moq);
        output.video_codec = Some("future_av1_encoder".into());
        let id = insert_output(&pool, 1, &output, false).await.unwrap();
        let stored = select_outputs(&pool, 1)
            .await
            .unwrap()
            .into_iter()
            .find(|output| output.id == id)
            .unwrap();
        assert_eq!(stored.video_codec.as_deref(), Some("future_av1_encoder"));
    }

    #[tokio::test]
    async fn parallel_instances_round_trip_and_invalid_ids_roll_back_the_selection() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let records = select_outputs(&pool, 1).await.unwrap();
        let primary = records.iter().find(|output| output.active).unwrap().clone();
        let mut config = crate::utils::config::PlayoutConfig {
            output: crate::utils::config::Output::from_record(primary.clone()),
            ..Default::default()
        };
        let mut stream =
            crate::utils::config::Output::from_record(Output::new(1, OutputMode::Stream));
        stream.stream_url = "rtmp://example.invalid/one".into();
        config.outputs.push(stream.clone());
        stream.stream_url = "rtmp://example.invalid/two".into();
        config.outputs.push(stream);
        let mut transaction = pool.begin().await.unwrap();
        save_outputs_on(&mut transaction, 1, &mut config)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_ne!(config.outputs[0].id, config.outputs[1].id);
        let selected = select_outputs(&pool, 1).await.unwrap();
        assert_eq!(selected.iter().filter(|output| output.active).count(), 3);
        for output in &config.outputs {
            assert!(
                selected
                    .iter()
                    .any(|record| record.id == output.id && record.stream_url == output.stream_url)
            );
        }
        let selected_id = super::super::select_configuration(&pool, 1)
            .await
            .unwrap()
            .output_id;
        assert_eq!(selected_id, primary.id);
        config.outputs[0].id = i32::MAX;
        let mut transaction = pool.begin().await.unwrap();
        assert!(
            save_outputs_on(&mut transaction, 1, &mut config)
                .await
                .is_err()
        );
        transaction.rollback().await.unwrap();
        assert_eq!(
            select_outputs(&pool, 1)
                .await
                .unwrap()
                .iter()
                .filter(|output| output.active)
                .count(),
            3
        );
    }

    #[tokio::test]
    async fn disabled_instances_names_and_program_survive_save_without_changing_enabled_outputs() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let primary = select_outputs(&pool, 1)
            .await
            .unwrap()
            .into_iter()
            .find(|output| output.active)
            .unwrap();
        let mut config = crate::utils::config::PlayoutConfig {
            output: crate::utils::config::Output::from_record(primary),
            program: Some(crate::utils::config::ProgramFormat {
                width: 1920,
                height: 1080,
                fps: 25.0,
            }),
            ..Default::default()
        };
        let mut additional =
            crate::utils::config::Output::from_record(Output::new(1, OutputMode::Stream));
        additional.label = "Streamingplattform".into();
        additional.public_moq = true;
        additional.enabled = false;
        additional.inherit_resolution = true;
        additional.share_encoding = false;
        additional.encoding_source_id = Some(config.output.id);
        additional.encoding_variant = "small".into();
        config.outputs.push(additional);
        let mut transaction = pool.begin().await.unwrap();
        save_outputs_on(&mut transaction, 1, &mut config)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let records = select_outputs(&pool, 1).await.unwrap();
        let saved = records
            .iter()
            .find(|output| output.id == config.outputs[0].id)
            .unwrap();
        assert!(!saved.active);
        assert!(saved.configured);
        assert!(saved.inherit_resolution);
        assert!(saved.share_encoding);
        assert_eq!(saved.encoding_source_id, Some(config.output.id));
        assert_eq!(saved.encoding_variant, "small");
        assert_eq!(saved.label, "Streamingplattform");
        assert!(saved.public_moq);
        assert!(crate::utils::config::Output::from_record(saved.clone()).public_moq);
        assert_eq!(records.iter().filter(|output| output.active).count(), 1);
        assert_eq!(
            select_program_format(&pool, 1)
                .await
                .unwrap()
                .unwrap()
                .width,
            1920
        );
        let restored = crate::utils::config::Output::from_record(saved.clone());
        assert!(!restored.enabled);
        assert!(restored.inherit_resolution);
        assert_eq!(restored.encoding_source_id, Some(config.output.id));
        assert_eq!(restored.encoding_variant, "small");
        assert_eq!(config.active_outputs().count(), 1);
    }

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
            "{\"copyright\":\"Example\"}",
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
        assert_eq!(selected.metadata_options, "{\"copyright\":\"Example\"}");
        assert_eq!(selected.audio_options, "{\"aac_coder\":\"fast\"}");
    }
}
