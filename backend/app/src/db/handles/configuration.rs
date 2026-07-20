use sqlx::{
    Executor, Sqlite,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{
    db::models::Configuration,
    utils::{config::PlayoutConfig, errors::ProcessError},
};

pub async fn select_configuration(
    pool: &SqlitePool,
    channel: i32,
) -> Result<Configuration, ProcessError> {
    const QUERY: &str = "SELECT * FROM configurations WHERE channel_id = $1";

    let result = sqlx::query_as(QUERY).bind(channel).fetch_one(pool).await?;

    Ok(result)
}

pub async fn insert_configuration<'e, E>(
    executor: E,
    channel_id: i32,
    output_id: i32,
) -> Result<SqliteQueryResult, ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    const QUERY: &str = "INSERT INTO configurations (channel_id, output_id) VALUES($1, $2)";

    let result = sqlx::query(QUERY)
        .bind(channel_id)
        .bind(output_id)
        .execute(executor)
        .await?;

    Ok(result)
}

pub async fn update_configuration(
    pool: &SqlitePool,
    id: i32,
    config: PlayoutConfig,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE configurations SET general_stop_threshold = $2, mail_subject = $3, mail_recipient = $4, mail_level = $5, mail_interval = $6, logging_ffmpeg_level = $7, logging_ingest_level = $8, logging_detect_silence = $9, logging_ignore = $10, processing_mode = $11, processing_add_logo = $12, processing_logo = $13, processing_logo_scale = $14, processing_logo_opacity = $15, processing_logo_position = $16, processing_volume = $17, processing_vtt_enable = $18, processing_vtt_dummy = $19, processing_vtt_name = $20, processing_vtt_language = $21, processing_vtt_default = $22, ingest_enable = $23, ingest_url = $24, playlist_day_start = $25, playlist_length = $26, playlist_infinit = $27, storage_filler = $28, storage_extensions = $29, storage_shuffle = $30, text_preset_id = $31, task_enable = $32, task_path = $33, output_id = $34 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(config.general.stop_threshold)
        .bind(config.mail.subject)
        .bind(config.mail.recipient)
        .bind(config.mail.mail_level.as_str())
        .bind(config.mail.interval)
        .bind(config.logging.ffmpeg_level)
        .bind(config.logging.ingest_level)
        .bind(config.logging.detect_silence)
        .bind(config.logging.ignore_lines.join(";"))
        .bind(config.processing.mode.to_string())
        .bind(config.processing.add_logo)
        .bind(config.processing.logo)
        .bind(config.processing.logo_scale)
        .bind(config.processing.logo_opacity)
        .bind(config.processing.logo_position)
        .bind(config.processing.volume)
        .bind(config.processing.vtt_enable)
        .bind(config.processing.vtt_dummy)
        .bind(config.processing.vtt_name)
        .bind(config.processing.vtt_language)
        .bind(config.processing.vtt_default)
        .bind(config.ingest.enable)
        .bind(config.ingest.ingest_url)
        .bind(config.playlist.day_start)
        .bind(config.playlist.length)
        .bind(config.playlist.infinit)
        .bind(config.storage.filler)
        .bind(config.storage.extensions.join(";"))
        .bind(config.storage.shuffle)
        .bind(config.text.preset_id)
        .bind(config.task.enable)
        .bind(config.task.path.to_string_lossy().to_string())
        .bind(config.output.id)
        .execute(pool)
        .await?;

    Ok(result)
}

pub async fn update_configuration_volume(
    pool: &SqlitePool,
    id: i32,
    volume: f64,
) -> Result<SqliteQueryResult, ProcessError> {
    const QUERY: &str = "UPDATE configurations SET processing_volume = $2 WHERE id = $1";

    let result = sqlx::query(QUERY)
        .bind(id)
        .bind(volume)
        .execute(pool)
        .await?;

    Ok(result)
}
