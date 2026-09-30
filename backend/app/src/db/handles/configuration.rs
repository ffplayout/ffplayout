use sqlx::{
    Executor, Row, Sqlite, SqliteConnection,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{
    db::models::{Configuration, LiveInputRecord},
    utils::{
        config::{LiveInput, PlayoutConfig, parse_rtmp_ingest_port},
        errors::ProcessError,
    },
};

pub async fn select_rtmp_ingest_urls<'e, E>(executor: E) -> Result<Vec<String>, ProcessError>
where
    E: Executor<'e, Database = Sqlite>,
{
    let urls = sqlx::query_scalar(
        "SELECT identifier FROM config_live_input
         WHERE backend = 'rtmp' AND takeover_mode = 'connection'",
    )
    .fetch_all(executor)
    .await?;

    Ok(urls)
}

pub async fn select_live_inputs(
    pool: &SqlitePool,
    config_id: i32,
) -> Result<Vec<LiveInput>, ProcessError> {
    let records = sqlx::query_as::<_, LiveInputRecord>(
        "SELECT id, priority, enabled, name, backend, identifier, options, demuxer_options FROM config_live_input
         WHERE config_id = $1 AND backend IN ('rtmp', 'srt') AND takeover_mode = 'connection'
         ORDER BY priority DESC, id",
    )
    .bind(config_id)
    .fetch_all(pool)
    .await?;

    records
        .into_iter()
        .map(|record| {
            Ok(LiveInput {
                id: record.id,
                priority: record.priority,
                enabled: record.enabled,
                name: record.name,
                backend: record.backend,
                identifier: record.identifier,
                options: serde_json::from_str(&record.options)?,
                demuxer_options: serde_json::from_str(&record.demuxer_options)?,
            })
        })
        .collect()
}

pub async fn select_configuration(
    pool: &SqlitePool,
    channel: i32,
) -> Result<Configuration, ProcessError> {
    const QUERY: &str = "SELECT
        c.id, c.channel_id,
        general.stop_threshold AS general_stop_threshold,
        mail.subject AS mail_subject, mail.recipient AS mail_recipient,
        mail.level AS mail_level, mail.interval AS mail_interval,
        notification.topic AS notification_topic,
        notification.level AS notification_level,
        notification.tags AS notification_tags,
        logging.ffmpeg_level AS logging_ffmpeg_level,
        logging.ingest_level AS logging_ingest_level,
        logging.detect_silence AS logging_detect_silence,
        logging.ignore_lines AS logging_ignore,
        processing.mode AS processing_mode,
        processing.add_logo AS processing_add_logo,
        processing.logo AS processing_logo,
        processing.logo_scale AS processing_logo_scale,
        processing.logo_opacity AS processing_logo_opacity,
        processing.logo_position AS processing_logo_position,
        audio.volume AS processing_volume,
        audio.live_loudness_enable AS processing_live_loudness_enable,
        audio.live_loudness_target_lufs AS processing_live_loudness_target_lufs,
        audio.live_loudness_dead_band_lu AS processing_live_loudness_dead_band_lu,
        audio.live_loudness_max_gain_db AS processing_live_loudness_max_gain_db,
        audio.live_loudness_max_attenuation_db AS processing_live_loudness_max_attenuation_db,
        audio.live_loudness_gain_up_db_per_second AS processing_live_loudness_gain_up_db_per_second,
        audio.live_loudness_gain_down_db_per_second AS processing_live_loudness_gain_down_db_per_second,
        audio.live_loudness_silence_gate_lufs AS processing_live_loudness_silence_gate_lufs,
        audio.live_loudness_true_peak_ceiling_dbtp AS processing_live_loudness_true_peak_ceiling_dbtp,
        processing.vtt_enable AS processing_vtt_enable,
        processing.vtt_dummy AS processing_vtt_dummy,
        processing.vtt_name AS processing_vtt_name,
        processing.vtt_language AS processing_vtt_language,
        processing.vtt_default AS processing_vtt_default,
        playlist.day_start AS playlist_day_start,
        playlist.length AS playlist_length,
        playlist.infinit AS playlist_infinit,
        storage.filler AS storage_filler,
        storage.extensions AS storage_extensions,
        storage.shuffle AS storage_shuffle,
        text.id AS text_preset_id,
        task.enable AS task_enable, task.path AS task_path,
        output.id AS output_id
    FROM config c
    JOIN config_general general ON general.config_id = c.id
    JOIN config_mail mail ON mail.config_id = c.id
    JOIN config_notification notification ON notification.config_id = c.id
    JOIN config_logging logging ON logging.config_id = c.id
    JOIN config_processing processing ON processing.config_id = c.id
    JOIN config_audio audio ON audio.config_id = c.id
    JOIN config_playlist playlist ON playlist.config_id = c.id
    JOIN config_storage storage ON storage.config_id = c.id
    LEFT JOIN config_text_presets text ON text.config_id = c.id AND text.persistent = 1
    JOIN config_task task ON task.config_id = c.id
    JOIN config_output output ON output.config_id = c.id AND output.active = 1
    WHERE c.channel_id = $1";

    Ok(sqlx::query_as(QUERY).bind(channel).fetch_one(pool).await?)
}

pub async fn insert_configuration(
    connection: &mut SqliteConnection,
    channel_id: i32,
    ingest_url: &str,
) -> Result<i32, ProcessError> {
    let config_id: i32 = sqlx::query("INSERT INTO config (channel_id) VALUES ($1) RETURNING id")
        .bind(channel_id)
        .fetch_one(&mut *connection)
        .await?
        .get("id");

    for query in [
        "INSERT INTO config_general (config_id) VALUES ($1)",
        "INSERT INTO config_mail (config_id) VALUES ($1)",
        "INSERT INTO config_notification (config_id) VALUES ($1)",
        "INSERT INTO config_logging (config_id) VALUES ($1)",
        "INSERT INTO config_processing (config_id) VALUES ($1)",
        "INSERT INTO config_audio (config_id) VALUES ($1)",
        "INSERT INTO config_playlist (config_id) VALUES ($1)",
        "INSERT INTO config_storage (config_id) VALUES ($1)",
        "INSERT INTO config_task (config_id) VALUES ($1)",
    ] {
        sqlx::query(query)
            .bind(config_id)
            .execute(&mut *connection)
            .await?;
    }

    sqlx::query(
        "INSERT INTO config_live_input
         (config_id, backend, identifier, takeover_mode)
         VALUES ($1, 'rtmp', $2, 'connection')",
    )
    .bind(config_id)
    .bind(ingest_url)
    .execute(&mut *connection)
    .await?;

    Ok(config_id)
}

pub async fn ingest_port_in_use(
    pool: &SqlitePool,
    channel_id: i32,
    port: u16,
) -> Result<bool, ProcessError> {
    live_listener_port_in_use(pool, channel_id, "rtmp", port).await
}

pub async fn live_listener_port_in_use(
    pool: &SqlitePool,
    channel_id: i32,
    backend: &str,
    port: u16,
) -> Result<bool, ProcessError> {
    let urls = sqlx::query_scalar::<_, String>(LIVE_LISTENER_URLS_QUERY)
        .bind(channel_id)
        .bind(backend)
        .fetch_all(pool)
        .await?;

    Ok(live_listener_urls_contain_port(&urls, backend, port))
}

const LIVE_LISTENER_URLS_QUERY: &str = "SELECT ingest.identifier FROM config
    JOIN config_live_input ingest ON ingest.config_id = config.id
    WHERE config.channel_id != $1 AND ingest.backend = $2
        AND ingest.takeover_mode = 'connection' AND ingest.enabled = 1";

pub async fn live_listener_port_in_use_on(
    connection: &mut SqliteConnection,
    channel_id: i32,
    backend: &str,
    port: u16,
) -> Result<bool, ProcessError> {
    let urls = sqlx::query_scalar::<_, String>(LIVE_LISTENER_URLS_QUERY)
        .bind(channel_id)
        .bind(backend)
        .fetch_all(&mut *connection)
        .await?;

    Ok(live_listener_urls_contain_port(&urls, backend, port))
}

fn live_listener_urls_contain_port(urls: &[String], backend: &str, port: u16) -> bool {
    urls.iter()
        .filter_map(|url| {
            if backend == "rtmp" {
                parse_rtmp_ingest_port(url).ok()
            } else {
                reqwest::Url::parse(url).ok().and_then(|url| url.port())
            }
        })
        .any(|configured_port| configured_port == port)
}

pub async fn update_configuration(
    pool: &SqlitePool,
    id: i32,
    config: PlayoutConfig,
) -> Result<SqliteQueryResult, ProcessError> {
    let mut transaction = pool.begin().await?;
    let result = update_configuration_on(&mut transaction, id, config).await?;
    transaction.commit().await?;
    Ok(result)
}

pub async fn update_configuration_on(
    connection: &mut SqliteConnection,
    id: i32,
    config: PlayoutConfig,
) -> Result<SqliteQueryResult, ProcessError> {
    sqlx::query("UPDATE config_general SET stop_threshold = $2 WHERE config_id = $1")
        .bind(id)
        .bind(config.general.stop_threshold)
        .execute(&mut *connection)
        .await?;
    sqlx::query("UPDATE config_mail SET subject = $2, recipient = $3, level = $4, interval = $5 WHERE config_id = $1")
        .bind(id).bind(config.mail.subject).bind(config.mail.recipient)
        .bind(config.mail.mail_level.as_str()).bind(config.mail.interval)
        .execute(&mut *connection).await?;
    sqlx::query(
        "UPDATE config_notification SET topic = $2, level = $3, tags = $4 WHERE config_id = $1",
    )
    .bind(id)
    .bind(config.notification.topic)
    .bind(config.notification.level.to_string())
    .bind(config.notification.tags)
    .execute(&mut *connection)
    .await?;
    sqlx::query("UPDATE config_logging SET ffmpeg_level = $2, ingest_level = $3, detect_silence = $4, ignore_lines = $5 WHERE config_id = $1")
        .bind(id).bind(config.logging.ffmpeg_level).bind(config.logging.ingest_level)
        .bind(config.logging.detect_silence).bind(config.logging.ignore_lines.join(";"))
        .execute(&mut *connection).await?;
    sqlx::query("UPDATE config_processing SET mode = $2, add_logo = $3, logo = $4, logo_scale = $5, logo_opacity = $6, logo_position = $7, vtt_enable = $8, vtt_dummy = $9, vtt_name = $10, vtt_language = $11, vtt_default = $12 WHERE config_id = $1")
        .bind(id).bind(config.processing.mode.to_string()).bind(config.processing.add_logo)
        .bind(config.processing.logo).bind(config.processing.logo_scale)
        .bind(config.processing.logo_opacity).bind(config.processing.logo_position)
        .bind(config.processing.vtt_enable).bind(config.processing.vtt_dummy)
        .bind(config.processing.vtt_name).bind(config.processing.vtt_language)
        .bind(config.processing.vtt_default).execute(&mut *connection).await?;
    sqlx::query("UPDATE config_audio SET volume = $2, live_loudness_enable = $3, live_loudness_target_lufs = $4, live_loudness_dead_band_lu = $5, live_loudness_max_gain_db = $6, live_loudness_max_attenuation_db = $7, live_loudness_gain_up_db_per_second = $8, live_loudness_gain_down_db_per_second = $9, live_loudness_silence_gate_lufs = $10, live_loudness_true_peak_ceiling_dbtp = $11 WHERE config_id = $1")
        .bind(id).bind(config.audio.volume).bind(config.audio.live_loudness_enable)
        .bind(config.audio.live_loudness_target_lufs).bind(config.audio.live_loudness_dead_band_lu)
        .bind(config.audio.live_loudness_max_gain_db).bind(config.audio.live_loudness_max_attenuation_db)
        .bind(config.audio.live_loudness_gain_up_db_per_second).bind(config.audio.live_loudness_gain_down_db_per_second)
        .bind(config.audio.live_loudness_silence_gate_lufs).bind(config.audio.live_loudness_true_peak_ceiling_dbtp)
        .execute(&mut *connection).await?;
    {
        let listeners = &config.ingest.listeners;
        let existing: Vec<i32> = sqlx::query_scalar(
            "SELECT id FROM config_live_input WHERE config_id = $1
             AND backend IN ('rtmp', 'srt') AND takeover_mode = 'connection'",
        )
        .bind(id)
        .fetch_all(&mut *connection)
        .await?;
        let mut retained = Vec::new();

        for listener in listeners {
            let listener_id = if listener.id > 0 {
                let result = sqlx::query(
                    "UPDATE config_live_input SET priority = $3, enabled = $4, name = $5,
                     backend = $6, identifier = $7, options = $8, demuxer_options = $9
                     WHERE id = $1 AND config_id = $2
                     AND backend IN ('rtmp', 'srt') AND takeover_mode = 'connection'",
                )
                .bind(listener.id)
                .bind(id)
                .bind(listener.priority)
                .bind(listener.enabled)
                .bind(&listener.name)
                .bind(&listener.backend)
                .bind(&listener.identifier)
                .bind(serde_json::to_string(&listener.options)?)
                .bind(serde_json::to_string(&listener.demuxer_options)?)
                .execute(&mut *connection)
                .await?;

                if result.rows_affected() != 1 {
                    return Err(ProcessError::Custom("unknown live listener".to_string()));
                }
                listener.id
            } else {
                sqlx::query_scalar(
                    "INSERT INTO config_live_input
                     (config_id, priority, enabled, name, backend, identifier, options, demuxer_options, takeover_mode)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'connection') RETURNING id",
                )
                .bind(id)
                .bind(listener.priority)
                .bind(listener.enabled)
                .bind(&listener.name)
                .bind(&listener.backend)
                .bind(&listener.identifier)
                .bind(serde_json::to_string(&listener.options)?)
                .bind(serde_json::to_string(&listener.demuxer_options)?)
                .fetch_one(&mut *connection)
                .await?
            };
            retained.push(listener_id);
        }

        for old_id in existing {
            if !retained.contains(&old_id) {
                sqlx::query("DELETE FROM config_live_input WHERE id = $1 AND config_id = $2")
                    .bind(old_id)
                    .bind(id)
                    .execute(&mut *connection)
                    .await?;
            }
        }
    }

    sqlx::query(
        "UPDATE config_playlist SET day_start = $2, length = $3, infinit = $4 WHERE config_id = $1",
    )
    .bind(id)
    .bind(config.playlist.day_start)
    .bind(config.playlist.length)
    .bind(config.playlist.infinit)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE config_storage SET filler = $2, extensions = $3, shuffle = $4 WHERE config_id = $1",
    )
    .bind(id)
    .bind(config.storage.filler)
    .bind(config.storage.extensions.join(";"))
    .bind(config.storage.shuffle)
    .execute(&mut *connection)
    .await?;
    activate_text_preset(connection, id, config.text.preset_id).await?;
    sqlx::query("UPDATE config_task SET enable = $2, path = $3 WHERE config_id = $1")
        .bind(id)
        .bind(config.task.enable)
        .bind(config.task.path.to_string_lossy().to_string())
        .execute(&mut *connection)
        .await?;

    activate_output(connection, id, config.output.id).await
}

async fn activate_output(
    connection: &mut SqliteConnection,
    config_id: i32,
    output_id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    // Clear the old selection first. The partial unique index then guarantees
    // that the second statement can never leave two active outputs behind.
    sqlx::query("UPDATE config_output SET active = 0 WHERE config_id = $1")
        .bind(config_id)
        .execute(&mut *connection)
        .await?;
    let result =
        sqlx::query("UPDATE config_output SET active = 1 WHERE config_id = $1 AND id = $2")
            .bind(config_id)
            .bind(output_id)
            .execute(&mut *connection)
            .await?;

    if result.rows_affected() != 1 {
        return Err(ProcessError::Custom(format!(
            "Output {output_id} does not belong to config {config_id}"
        )));
    }

    Ok(result)
}

async fn activate_text_preset(
    connection: &mut SqliteConnection,
    config_id: i32,
    preset_id: Option<i32>,
) -> Result<(), ProcessError> {
    sqlx::query("UPDATE config_text_presets SET persistent = 0 WHERE config_id = $1")
        .bind(config_id)
        .execute(&mut *connection)
        .await?;

    if let Some(preset_id) = preset_id {
        let result = sqlx::query(
            "UPDATE config_text_presets SET persistent = 1 WHERE config_id = $1 AND id = $2",
        )
        .bind(config_id)
        .bind(preset_id)
        .execute(&mut *connection)
        .await?;
        if result.rows_affected() != 1 {
            return Err(ProcessError::Custom(format!(
                "Text preset {preset_id} does not belong to config {config_id}"
            )));
        }
    }

    Ok(())
}

pub async fn update_configuration_volume(
    pool: &SqlitePool,
    id: i32,
    volume: f64,
) -> Result<SqliteQueryResult, ProcessError> {
    Ok(
        sqlx::query("UPDATE config_audio SET volume = $2 WHERE config_id = $1")
            .bind(id)
            .bind(volume)
            .execute(pool)
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::handles::db_migrate;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn migration_preserves_non_default_configuration_data() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Seed the last schema before configuration splitting, then let the
        // regular migrator apply every later migration.
        sqlx::migrate!("../../migrations")
            .run_to(2, &pool)
            .await
            .unwrap();
        sqlx::raw_sql(
            "UPDATE configurations SET general_stop_threshold = 17.5,
                mail_subject = 'Migrated subject', logging_ignore = 'custom warning',
                processing_logo = 'custom/logo.png', ingest_enable = 1,
                ingest_url = 'rtmp://127.0.0.1:1940/live/test',
                playlist_length = '12:00:00', storage_filler = 'custom/filler.mp4',
                text_preset_id = 4, task_enable = 1, task_path = '/opt/task', output_id = 2;
             UPDATE audio_config SET volume = 0.42, live_loudness_enable = 1;
             UPDATE outputs SET width = 1920, height = 1080 WHERE id = 2;
             UPDATE recordings SET enabled = 1, source = 'encode', width = 640, height = 360;",
        )
        .execute(&pool)
        .await
        .unwrap();
        db_migrate(&pool).await.unwrap();
        let config = select_configuration(&pool, 1).await.unwrap();
        assert_eq!(config.general_stop_threshold, 17.5);
        assert_eq!(config.mail_subject, "Migrated subject");
        assert_eq!(config.notification_level, "FATAL");
        assert_eq!(config.logging_ignore, "custom warning");
        assert_eq!(config.processing_logo, "custom/logo.png");
        assert_eq!(config.processing_volume, 0.42);
        assert!(config.processing_live_loudness_enable);
        let migrated_listener: (bool, String, i32) = sqlx::query_as(
            "SELECT enabled, identifier, priority FROM config_live_input
             WHERE config_id = $1 AND backend = 'rtmp' AND takeover_mode = 'connection'",
        )
        .bind(config.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(migrated_listener.0);
        assert_eq!(migrated_listener.1, "rtmp://127.0.0.1:1940/live/test");
        assert_eq!(migrated_listener.2, 100);
        assert_eq!(config.playlist_length, "12:00:00");
        assert_eq!(config.storage_filler, "custom/filler.mp4");
        assert_eq!(config.text_preset_id, Some(4));
        assert!(config.task_enable);
        assert_eq!(config.task_path, "/opt/task");
        assert_eq!(config.output_id, 2);

        let output = super::super::select_outputs(&pool, 1)
            .await
            .unwrap()
            .into_iter()
            .find(|output| output.id == 2)
            .unwrap();
        assert_eq!((output.width, output.height), (1920, 1080));
        let recording = super::super::select_recording(&pool, 1).await.unwrap();
        assert!(recording.enabled);
        assert_eq!(recording.source, "encode");
        assert_eq!((recording.width, recording.height), (640, 360));

        let old_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN
             ('global', 'roles', 'user', 'user_channels', 'refresh_tokens',
              'outputs', 'configurations', 'audio_config', 'recordings', 'text_presets',
              'config_ingest')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_tables, 0);
    }

    #[tokio::test]
    async fn migration_preserves_one_active_output_and_enforces_uniqueness() {
        let pool = pool().await;
        let active: Vec<i32> =
            sqlx::query_scalar("SELECT id FROM config_output WHERE config_id = 1 AND active = 1")
                .fetch_all(&pool)
                .await
                .unwrap();

        assert_eq!(active, [1]);
        assert!(
            sqlx::query("UPDATE config_output SET active = 1 WHERE id = 2")
                .execute(&pool)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn future_pipeline_configuration_has_safe_defaults_and_audio_tracks_are_ordered() {
        let pool = pool().await;
        let output_id: i32 =
            sqlx::query_scalar("SELECT id FROM config_output WHERE config_id = 1 AND active = 1")
                .fetch_one(&pool)
                .await
                .unwrap();

        let output_options: (String, String, String, String, String) = sqlx::query_as(
            "SELECT audio_options, protocol_options, device_backend, device_identifier, device_options
             FROM config_output WHERE id = $1",
        )
        .bind(output_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            output_options,
            (
                "{}".to_string(),
                "{}".to_string(),
                String::new(),
                String::new(),
                "{}".to_string(),
            )
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT program_layout FROM config_audio WHERE config_id = 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "stereo"
        );
        assert_eq!(
            sqlx::query_as::<_, (String, String)>(
                "SELECT demuxer_options, protocol_options FROM config_source WHERE config_id = 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            ("{}".to_string(), "{}".to_string())
        );

        let rtmp_input: (i64, String, String, String) = sqlx::query_as(
            "SELECT enabled, backend, identifier, takeover_mode
             FROM config_live_input
             WHERE config_id = 1 AND backend = 'rtmp' AND takeover_mode = 'connection'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            rtmp_input,
            (
                0,
                "rtmp".to_string(),
                "rtmp://127.0.0.1:1936/live/stream".to_string(),
                "connection".to_string(),
            )
        );

        let live_input: (i64, String, f64, f64) = sqlx::query_as(
            "INSERT INTO config_live_input (config_id, priority, backend)
             VALUES (1, 50, 'ndi')
             RETURNING enabled, takeover_mode, signal_loss_grace_seconds, max_duration_seconds",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live_input, (0, "manual".to_string(), 5.0, 0.0));
        let ndi_id: i32 = sqlx::query_scalar(
            "SELECT id FROM config_live_input WHERE config_id = 1 AND backend = 'ndi'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query("INSERT INTO config_live_input (config_id, priority, backend) VALUES (1, 101, 'ndi')")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE config_live_input SET priority = 101 WHERE id = $1")
                .bind(ndi_id)
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("UPDATE config_live_input SET priority = 100 WHERE id = $1")
            .bind(ndi_id)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO config_live_input (config_id, backend, takeover_mode)
             VALUES (1, 'rtmp', 'connection')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO config_output_audio (output_id, position, language, channel_layout)
             VALUES ($1, 0, 'deu', '5.1')",
        )
        .bind(output_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query("INSERT INTO config_output_audio (output_id, position) VALUES ($1, 0)")
                .bind(output_id)
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query(
            "INSERT INTO config_output_audio (output_id, position, default_track)
             VALUES ($1, 1, 1)",
        )
        .bind(output_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "INSERT INTO config_output_audio (output_id, position, default_track)
                 VALUES ($1, 2, 1)",
            )
            .bind(output_id)
            .execute(&pool)
            .await
            .is_err()
        );
        sqlx::query("INSERT INTO config_output_audio (output_id, position) VALUES ($1, 2)")
            .bind(output_id)
            .execute(&pool)
            .await
            .unwrap();

        let transient_output_id: i32 = sqlx::query_scalar(
            "INSERT INTO config_output (config_id, name) VALUES (1, 'test') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO config_output_audio (output_id, position, default_track)
             VALUES ($1, 0, 1)",
        )
        .bind(transient_output_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM config_output WHERE id = $1")
            .bind(transient_output_id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM config_output_audio WHERE output_id = $1",
            )
            .bind(transient_output_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn output_selection_is_switched_atomically() {
        let pool = pool().await;
        let mut transaction = pool.begin().await.unwrap();
        activate_output(&mut transaction, 1, 2).await.unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(select_configuration(&pool, 1).await.unwrap().output_id, 2);

        let mut transaction = pool.begin().await.unwrap();
        assert!(
            activate_output(&mut transaction, 1, i32::MAX)
                .await
                .is_err()
        );
        transaction.rollback().await.unwrap();
        assert_eq!(select_configuration(&pool, 1).await.unwrap().output_id, 2);
    }

    #[tokio::test]
    async fn text_preset_selection_is_unique_and_can_be_cleared() {
        let pool = pool().await;
        assert_eq!(
            select_configuration(&pool, 1).await.unwrap().text_preset_id,
            None
        );

        let mut transaction = pool.begin().await.unwrap();
        activate_text_preset(&mut transaction, 1, Some(1))
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            select_configuration(&pool, 1).await.unwrap().text_preset_id,
            Some(1)
        );
        assert!(
            sqlx::query("UPDATE config_text_presets SET persistent = 1 WHERE id = 2")
                .execute(&pool)
                .await
                .is_err()
        );

        let mut transaction = pool.begin().await.unwrap();
        activate_text_preset(&mut transaction, 1, None)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            select_configuration(&pool, 1).await.unwrap().text_preset_id,
            None
        );
    }

    #[tokio::test]
    async fn complete_configuration_update_rolls_back_all_topics_on_error() {
        let pool = pool().await;
        sqlx::raw_sql(
            "UPDATE config_global SET logs = 'assets', playlists = 'assets',
                public = 'assets', storage = 'assets';
             UPDATE channels SET public = 'assets', playlists = 'assets', storage = 'assets';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
        let original = select_configuration(&pool, 1).await.unwrap();
        config.general.stop_threshold = 23.5;
        config.mail.subject = "Should roll back".to_string();
        config.notification.topic = "should-roll-back".to_string();
        config.audio.volume = 0.25;
        config.output.id = i32::MAX;

        assert!(update_configuration(&pool, 1, config).await.is_err());
        let unchanged = select_configuration(&pool, 1).await.unwrap();
        assert_eq!(
            unchanged.general_stop_threshold,
            original.general_stop_threshold
        );
        assert_eq!(unchanged.mail_subject, original.mail_subject);
        assert_eq!(unchanged.notification_topic, original.notification_topic);
        assert_eq!(unchanged.processing_volume, original.processing_volume);
        assert_eq!(unchanged.output_id, original.output_id);
    }

    #[tokio::test]
    async fn live_listener_configuration_recreates_a_missing_listener_row() {
        let pool = pool().await;
        sqlx::raw_sql(
            "UPDATE config_global SET logs = 'assets', playlists = 'assets',
                public = 'assets', storage = 'assets';
             UPDATE channels SET public = 'assets', playlists = 'assets', storage = 'assets';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut config = PlayoutConfig::new(&pool, 1, None).await.unwrap();

        sqlx::query(
            "DELETE FROM config_live_input
             WHERE config_id = 1 AND backend = 'rtmp' AND takeover_mode = 'connection'",
        )
        .execute(&pool)
        .await
        .unwrap();
        config.ingest.listeners = vec![crate::utils::config::LiveInput {
            enabled: true,
            backend: "rtmp".to_string(),
            identifier: "rtmp://127.0.0.1:1940/live/recreated".to_string(),
            ..Default::default()
        }];
        update_configuration(&pool, 1, config).await.unwrap();

        let restored = PlayoutConfig::new(&pool, 1, None).await.unwrap();
        assert!(restored.ingest.listeners[0].enabled);
        assert_eq!(
            restored.ingest.listeners[0].identifier,
            "rtmp://127.0.0.1:1940/live/recreated"
        );
    }

    #[tokio::test]
    async fn multiple_live_listeners_roundtrip_without_changing_the_rtmp_id() {
        let pool = pool().await;
        sqlx::raw_sql(
            "UPDATE config_global SET logs = 'assets', playlists = 'assets',
                public = 'assets', storage = 'assets';
             UPDATE channels SET public = 'assets', playlists = 'assets', storage = 'assets';",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO config_live_input (config_id, backend, takeover_mode)
             VALUES (1, 'ndi', 'manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
        let listeners = &mut config.ingest.listeners;
        let rtmp_id = listeners[0].id;
        listeners[0].enabled = true;
        listeners[0].priority = 10;
        listeners[0]
            .demuxer_options
            .insert("format".to_string(), "live_flv".to_string());
        listeners.push(crate::utils::config::LiveInput {
            enabled: true,
            backend: "srt".to_string(),
            identifier: "srt://127.0.0.1:9000".to_string(),
            priority: 20,
            options: std::collections::BTreeMap::from([
                ("passphrase".to_string(), "secret-passphrase".to_string()),
                ("pbkeylen".to_string(), "16".to_string()),
            ]),
            demuxer_options: std::collections::BTreeMap::from([
                ("format".to_string(), "mpegts".to_string()),
                ("scan_all_pmts".to_string(), "1".to_string()),
            ]),
            ..Default::default()
        });
        update_configuration(&pool, 1, config).await.unwrap();

        let mut restored = PlayoutConfig::new(&pool, 1, None).await.unwrap();
        let listeners = &restored.ingest.listeners;
        assert_eq!(listeners.len(), 2);
        assert_eq!(listeners[0].backend, "srt");
        assert_eq!(listeners[0].options["passphrase"], "secret-passphrase");
        assert_eq!(listeners[0].demuxer_options["format"], "mpegts");
        assert_eq!(listeners[0].demuxer_options["scan_all_pmts"], "1");
        assert_eq!(listeners[1].id, rtmp_id);
        assert_eq!(listeners[1].priority, 10);
        assert_eq!(listeners[1].demuxer_options["format"], "live_flv");

        restored
            .ingest
            .listeners
            .retain(|listener| listener.backend == "rtmp");
        update_configuration(&pool, 1, restored).await.unwrap();
        let final_config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
        assert_eq!(final_config.ingest.listeners[0].id, rtmp_id);
        let ndi_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM config_live_input WHERE config_id = 1 AND backend = 'ndi'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ndi_count, 1);
    }

    #[tokio::test]
    async fn listener_port_check_uses_the_saving_transaction() {
        let pool = pool().await;
        sqlx::query("INSERT INTO channels (id, name, preview_url) VALUES (2, 'Other', '')")
            .execute(&pool)
            .await
            .unwrap();
        let other_config_id: i32 =
            sqlx::query_scalar("INSERT INTO config (channel_id) VALUES (2) RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO config_live_input (config_id, enabled, backend, identifier, takeover_mode)
             VALUES ($1, 1, 'srt', 'srt://127.0.0.1:9000', 'connection')",
        )
        .bind(other_config_id)
        .execute(&pool)
        .await
        .unwrap();
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();

        assert!(
            live_listener_port_in_use_on(&mut transaction, 1, "srt", 9000)
                .await
                .unwrap()
        );
        assert!(
            !live_listener_port_in_use_on(&mut transaction, 2, "srt", 9000)
                .await
                .unwrap()
        );
        transaction.rollback().await.unwrap();
    }
}
