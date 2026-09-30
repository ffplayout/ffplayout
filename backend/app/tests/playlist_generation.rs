use std::{fs, path::PathBuf};

use chrono::NaiveTime;
use sqlx::sqlite::SqlitePoolOptions;

use ffplayout::{
    db::handles,
    player::{controller::ChannelManager, utils::*},
    utils::{
        config::{PlayoutConfig, ProcessMode::Playlist, Source, Template},
        generator::*,
        playlist::generate_playlist,
        system::SystemStat,
    },
};

async fn prepare_config() -> (PlayoutConfig, ChannelManager) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    let current_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets")
        .canonicalize()
        .unwrap();
    let hls = current_path.join("hls");
    let log = current_path.join("log");
    let playlists = current_path.join("playlists");
    let storage = current_path.join("storage");
    let filler = current_path.join("storage/media_filler/filler_0.mp4");

    sqlx::query(
        r#"
        UPDATE config_global SET public = $1, logs = $2, playlists = $3, storage = $4;
        UPDATE channels SET public = $1, playlists = $3, storage = $4;
        UPDATE config_storage SET filler = $5;
        UPDATE config_output SET width = 1024, height = 576, active = 0;
        UPDATE config_output SET active = 1 WHERE id = 4;
        "#,
    )
    .bind(hls.to_string_lossy())
    .bind(log.to_string_lossy())
    .bind(playlists.to_string_lossy())
    .bind(storage.to_string_lossy())
    .bind(filler.to_string_lossy())
    .execute(&pool)
    .await
    .unwrap();

    let config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(
        pool,
        channel,
        config.clone(),
        tokio_util::sync::CancellationToken::new(),
        SystemStat::new(),
    )
    .await
    .expect("test storage should initialize");

    (config, manager)
}

#[tokio::test]
#[ignore]
async fn test_filler_list() {
    let (mut config, manager) = prepare_config().await;

    let current_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets")
        .canonicalize()
        .unwrap();

    config.storage.filler = "storage/media_filler".into();
    config.storage.filler_path = current_path.join("storage/media_filler");

    let f_list = filler_list(&config, &manager, 2440.0).await;

    assert_eq!(sum_durations(&f_list), 2440.0);
}

#[tokio::test]
#[ignore]
async fn test_generate_playlist_from_folder() {
    let (mut config, manager) = prepare_config().await;

    let current_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets")
        .canonicalize()
        .unwrap();

    config.general.generate = Some(vec!["2023-09-11".to_string()]);
    config.processing.mode = Playlist;
    config.storage.filler = "storage/media_filler".into();
    config.storage.filler_path = current_path.join("storage/media_filler");
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    let playlist = generate_playlist(manager, None).await;
    let path_1 = current_path.join("playlists/2023/09/2023-09-11.json");

    assert!(playlist.is_ok());
    assert!(path_1.is_file());

    let total_duration = sum_durations(&playlist.unwrap().program);

    assert!(
        total_duration > 86399.0 && total_duration < 86401.0,
        "total_duration is {total_duration}"
    );

    fs::remove_file(path_1).expect("Delete test playlist");
}

#[tokio::test]
#[ignore]
async fn test_generate_playlist_from_template() {
    let (mut config, manager) = prepare_config().await;

    let current_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets")
        .canonicalize()
        .unwrap();

    config.general.generate = Some(vec!["2023-09-12".to_string()]);
    config.general.template = Some(Template {
        sources: vec![
            Source {
                start: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                duration: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                shuffle: false,
                paths: vec![current_path.join("storage")],
            },
            Source {
                start: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                duration: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                shuffle: true,
                paths: vec![current_path.join("storage")],
            },
        ],
    });
    config.processing.mode = Playlist;
    config.storage.filler = "storage/media_filler".into();
    config.storage.filler_path = current_path.join("storage/media_filler");
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = current_path.join("playlists");

    manager.update_config(config).await;

    let playlist = generate_playlist(manager, None).await;

    let path_1 = current_path.join("playlists/2023/09/2023-09-12.json");

    assert!(path_1.is_file());

    assert!(playlist.is_ok());

    let total_duration = sum_durations(&playlist.unwrap().program);

    assert!(
        total_duration > 86399.0 && total_duration < 86401.0,
        "total_duration is {total_duration}"
    );

    fs::remove_file(path_1).expect("Delete test playlist");
}
