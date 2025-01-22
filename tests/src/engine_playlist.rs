use serial_test::serial;
use sqlx::sqlite::SqlitePoolOptions;

use ffplayout::db::handles;
use ffplayout::player::output::player;
use ffplayout::player::{controller::ChannelManager, input::playlist::gen_source, utils::Media};
use ffplayout::utils::config::OutputMode::Null;
use ffplayout::utils::config::{PlayoutConfig, ProcessMode::Playlist};
use ffplayout::utils::time_machine::set_mock_time;
use ffplayout::vec_strings;

async fn prepare_config() -> (PlayoutConfig, ChannelManager) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    sqlx::query(
        r#"
        UPDATE global SET public = "assets/hls", logs = "assets/log", playlists = "assets/playlists", storage = "assets/storage";
        UPDATE channels SET public = "assets/hls", playlists = "assets/playlists", storage = "assets/storage";
        UPDATE configurations SET processing_width = 1024, processing_height = 576;
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let config = PlayoutConfig::new(&pool, 1).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(pool, channel, config.clone());

    (config, manager)
}

async fn timed_stop(sec: u64, manager: ChannelManager) {
    tokio::time::sleep(tokio::time::Duration::from_secs(sec)).await;

    println!("Timed stop of process");

    manager.channel.lock().await.active = false;
    manager.stop_all(false).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_gen_source() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();

    let mut valid_source_with_probe = Media::new(0, "assets/media_mix/av_sync.mp4", true).await;
    let valid_media = gen_source(&config, valid_source_with_probe.clone(), &manager, 100).await;

    assert_eq!(valid_source_with_probe.source, valid_media.source);

    let mut valid_source_without_probe = Media::new(0, "assets/media_mix/av_sync.mp4", false).await;
    valid_source_without_probe.duration = 30.0;
    valid_source_without_probe.out = 20.0;
    let valid_media = gen_source(&config, valid_source_without_probe.clone(), &manager, 100).await;

    assert_eq!(valid_source_without_probe.source, valid_media.source);
    assert_eq!(valid_media.out, 20.0);

    valid_source_with_probe.out = 0.9;

    let valid_media = gen_source(&config, valid_source_with_probe.clone(), &manager, 100).await;

    assert!(valid_media.skip);

    let mut no_valid_source_with_probe = Media::new(0, "assets/media_mix/av_snc.mp4", true).await;
    no_valid_source_with_probe.duration = 30.0;
    no_valid_source_with_probe.out = 30.0;

    let valid_media = gen_source(&config, no_valid_source_with_probe.clone(), &manager, 100).await;

    assert_eq!(valid_media.source, "color=c=#121212:s=1024x576:d=30");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_missing() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();
    config.output.mode = Null;
    config.output.output_count = 1;
    config.output.output_filter = None;
    config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    manager.update_config(config).await;
    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-07T23:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    player(manager.clone()).await.unwrap();

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-08");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_next_missing() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();
    config.output.mode = Null;
    config.output.output_count = 1;
    config.output.output_filter = None;
    config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    manager.update_config(config).await;
    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-09T23:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    player(manager.clone()).await.unwrap();

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-10");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_to_short() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();
    config.output.mode = Null;
    config.output.output_count = 1;
    config.output.output_filter = None;
    config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    manager.update_config(config).await;
    let manager_clone = manager.clone();

    set_mock_time(&Some("2024-01-31T05:59:40+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    player(manager.clone()).await.unwrap();

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2024-01-31");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_init_after_list_end() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();
    config.output.mode = Null;
    config.output.output_count = 1;
    config.output.output_filter = None;
    config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    manager.update_config(config).await;
    let manager_clone = manager.clone();

    set_mock_time(&Some("2024-01-31T05:59:47+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    player(manager.clone()).await.unwrap();

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2024-01-31");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_change_at_midnight() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();
    config.output.mode = Null;
    config.output.output_count = 1;
    config.output.output_filter = None;
    config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    manager.update_config(config).await;
    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-08T23:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    player(manager.clone()).await.unwrap();

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-09");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_change_before_midnight() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "23:59:45".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();
    config.output.mode = Null;
    config.output.output_count = 1;
    config.output.output_filter = None;
    config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    manager.update_config(config).await;
    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-08T23:59:30+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(35, manager_clone));

    player(manager.clone()).await.unwrap();

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-09");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_change_at_six() {
    let (mut config, manager) = prepare_config().await;

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;
    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);
    config.channel.playlists = "assets/playlists".into();
    config.storage.filler = "assets/media_filler/filler_0.mp4".into();
    config.output.mode = Null;
    config.output.output_count = 1;
    config.output.output_filter = None;
    config.output.output_cmd = Some(vec_strings!["-f", "null", "-"]);

    manager.update_config(config).await;
    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-09T05:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    player(manager.clone()).await.unwrap();

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-09");
}
