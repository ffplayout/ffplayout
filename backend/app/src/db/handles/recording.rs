use sqlx::{
    SqliteConnection,
    sqlite::{SqlitePool, SqliteQueryResult},
};

use crate::{
    db::models::Recording,
    utils::{
        config::{Recording as RecordingConfig, RecordingSource},
        errors::ProcessError,
    },
};

pub async fn select_recording(
    pool: &SqlitePool,
    channel_id: i32,
) -> Result<Recording, ProcessError> {
    Ok(sqlx::query_as(
        "SELECT recording.config_id AS id, recording.*, config.channel_id
            FROM config_recording recording
            JOIN config ON config.id = recording.config_id
            WHERE config.channel_id = $1",
    )
    .bind(channel_id)
    .fetch_one(pool)
    .await?)
}

pub async fn insert_recording(
    connection: &mut SqliteConnection,
    config_id: i32,
    channel_id: i32,
) -> Result<SqliteQueryResult, ProcessError> {
    // Each channel gets its own recording directory by default so that
    // multiple channels never write their segments into the same folder.
    let default_path = format!("/var/lib/ffplayout/recordings/{channel_id}");
    Ok(
        sqlx::query("INSERT INTO config_recording (config_id, path) VALUES ($1, $2)")
            .bind(config_id)
            .bind(default_path)
            .execute(connection)
            .await?,
    )
}

pub async fn update_recording(
    pool: &SqlitePool,
    channel_id: i32,
    recording: &RecordingConfig,
) -> Result<SqliteQueryResult, ProcessError> {
    let mut connection = pool.acquire().await?;
    update_recording_on(&mut connection, channel_id, recording).await
}

pub async fn update_recording_on(
    connection: &mut SqliteConnection,
    channel_id: i32,
    recording: &RecordingConfig,
) -> Result<SqliteQueryResult, ProcessError> {
    let source = match recording.source {
        RecordingSource::HlsVariant => "hls_variant",
        RecordingSource::Stream => "stream",
        RecordingSource::Encode => "encode",
    };
    let video_options = serde_json::to_string(&recording.video_options)?;
    Ok(sqlx::query("UPDATE config_recording SET enabled = $2, source = $3, source_output_id = $4, hls_variant = $5, path = $6, segment_duration = $7, retention_days = $8, minimum_free_space_gb = $9, width = $10, height = $11, video_codec = $12, video_options = $13, audio_codec = $14, audio_bitrate = $15 WHERE config_id = (SELECT id FROM config WHERE channel_id = $1)")
        .bind(channel_id).bind(recording.enable).bind(source).bind(recording.source_output_id).bind(&recording.variant).bind(&recording.path)
        .bind(i64::from(recording.segment_duration)).bind(i64::from(recording.retention_days)).bind(i64::from(recording.minimum_free_space_gb))
        .bind(i64::from(recording.width)).bind(i64::from(recording.height)).bind(&recording.video_codec).bind(video_options).bind(&recording.audio_codec).bind(i64::from(recording.audio_bitrate))
        .execute(connection).await?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::handles::db_migrate;

    #[tokio::test]
    async fn recording_update_and_select_are_scoped_to_the_channel() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        let recording = RecordingConfig {
            enable: true,
            source: RecordingSource::Encode,
            source_output_id: None,
            variant: String::new(),
            path: "/srv/recordings/channel-1".to_string(),
            segment_duration: 120,
            retention_days: 14,
            minimum_free_space_gb: 5,
            width: 640,
            height: 360,
            video_codec: "libx264".to_string(),
            video_options: BTreeMap::from([("preset".to_string(), "fast".to_string())]),
            audio_codec: "aac".to_string(),
            audio_bitrate: 192,
        };

        assert_eq!(
            update_recording(&pool, 999, &recording)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        assert_eq!(
            update_recording(&pool, 1, &recording)
                .await
                .unwrap()
                .rows_affected(),
            1
        );

        let selected = select_recording(&pool, 1).await.unwrap();
        assert_eq!(selected.channel_id, 1);
        assert!(selected.enabled);
        assert_eq!(selected.source, "encode");
        assert_eq!(selected.path, "/srv/recordings/channel-1");
        assert_eq!(selected.segment_duration, 120);
        assert_eq!((selected.width, selected.height), (640, 360));
        assert_eq!(selected.audio_bitrate, 192);
    }
}
