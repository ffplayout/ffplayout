use std::env;

use chrono::NaiveTime;
use sqlx::sqlite::SqlitePoolOptions;

use ffplayout::db::handles;
use ffplayout::player::{controller::ChannelManager, utils::*};
use ffplayout::utils::config::ProcessMode::Playlist;
use ffplayout::utils::playlist::generate_playlist;
use ffplayout::utils::{
    config::{PlayoutConfig, Source, Template},
    generator::*,
};

async fn prepare_config() -> (PlayoutConfig, ChannelManager) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    let current_path = env::current_dir().unwrap();
    let hls = current_path.join("assets/hls");
    let log = current_path.join("assets/log");
    let playlists = current_path.join("assets/playlists");
    let storage = current_path.join("assets/storage");
    let filler = current_path.join("assets/storage/media_filler/filler_0.mp4");

    sqlx::query(
        r#"
        UPDATE global SET public = $1, logs = $2, playlists = $3, storage = $4;
        UPDATE channels SET public = $1, playlists = $3, storage = $4;
        UPDATE configurations SET processing_width = 1024, processing_height = 576, storage_filler = $5, output_id = 4;
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
    let manager = ChannelManager::new(pool, channel, config.clone()).await;

    (config, manager)
}

#[tokio::test]
async fn test_random_list() {
    let clip_list = vec![
        Media::new(0, "./assets/storage/media_mix/with_audio.mp4", true).await, // 30 seconds
        Media::new(0, "./assets/storage/media_mix/dual_audio.mp4", true).await, // 30 seconds
        Media::new(0, "./assets/storage/media_mix/av_sync.mp4", true).await,    // 30 seconds
        Media::new(0, "./assets/storage/media_mix/ad.mp4", true).await,         // 25 seconds
    ];

    let r_list = random_list(clip_list.clone(), 200.0);
    let r_duration = sum_durations(&r_list);

    assert!(200.0 >= r_duration, "duration is {r_duration}");
    assert!(r_duration >= 170.0);
}

#[tokio::test]
async fn test_ordered_list() {
    let clip_list = vec![
        Media::new(0, "./assets/storage/media_mix/with_audio.mp4", true).await, // 30 seconds
        Media::new(0, "./assets/storage/media_mix/dual_audio.mp4", true).await, // 30 seconds
        Media::new(0, "./assets/storage/media_mix/av_sync.mp4", true).await,    // 30 seconds
        Media::new(0, "./assets/storage/media_mix/ad.mp4", true).await,         // 25 seconds
    ];

    let o_list = ordered_list(clip_list.clone(), 85.0);

    assert_eq!(o_list.len(), 3);
    assert_eq!(o_list[2].duration, 25.0);
    assert_eq!(sum_durations(&o_list), 85.0);

    let o_list = ordered_list(clip_list, 120.0);

    assert_eq!(o_list.len(), 4);
    assert_eq!(o_list[2].duration, 30.0);
    assert_eq!(sum_durations(&o_list), 115.0);
}

#[tokio::test]
#[ignore]
async fn test_filler_list() {
    let (mut config, manager) = prepare_config().await;

    let current_path = env::current_dir().unwrap();

    config.storage.filler = "assets/storage/media_filler".into();
    config.storage.filler_path = current_path.join("assets/storage/media_filler");

    let f_list = filler_list(&config, &manager, 2440.0).await;

    assert_eq!(sum_durations(&f_list), 2440.0);
}

#[tokio::test]
#[ignore]
async fn test_generate_playlist_from_folder() {
    let (mut config, manager) = prepare_config().await;

    let current_path = env::current_dir().unwrap();

    config.general.generate = Some(vec!["2023-09-11".to_string()]);
    config.processing.mode = Playlist;
    config.storage.filler = "assets/storage/media_filler".into();
    config.storage.filler_path = current_path.join("assets/storage/media_filler");
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    let playlist = generate_playlist(manager).await;

    assert!(playlist.is_ok());

    let total_duration = sum_durations(&playlist.unwrap().program);

    assert!(
        total_duration > 86399.0 && total_duration < 86401.0,
        "total_duration is {total_duration}"
    );
}

#[tokio::test]
#[ignore]
async fn test_generate_playlist_from_template() {
    let (mut config, manager) = prepare_config().await;

    let current_path = env::current_dir().unwrap();

    config.general.generate = Some(vec!["2023-09-12".to_string()]);
    config.general.template = Some(Template {
        sources: vec![
            Source {
                start: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                duration: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                shuffle: false,
                paths: vec![current_path.join("assets/storage")],
            },
            Source {
                start: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                duration: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
                shuffle: true,
                paths: vec![current_path.join("assets/storage")],
            },
        ],
    });
    config.processing.mode = Playlist;
    config.storage.filler = "assets/storage/media_filler".into();
    config.storage.filler_path = current_path.join("assets/storage/media_filler");
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();

    manager.update_config(config).await;

    let playlist = generate_playlist(manager).await;

    assert!(playlist.is_ok());

    let total_duration = sum_durations(&playlist.unwrap().program);

    assert!(
        total_duration > 86399.0 && total_duration < 86401.0,
        "total_duration is {total_duration}"
    );
}
