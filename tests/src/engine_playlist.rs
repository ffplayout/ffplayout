use std::{env, sync::atomic::Ordering};

use serial_test::serial;
use sqlx::sqlite::SqlitePoolOptions;

use ffplayout::db::handles;
use ffplayout::player::controller::ChannelManager;
use ffplayout::player::output::player;
use ffplayout::utils::config::{PlayoutConfig, ProcessMode::Playlist};
use ffplayout::utils::time_machine::set_mock_time;

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

    let mut config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();

    config.general.skip_validation = true;
    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.ingest.enable = false;
    config.text.add_text = false;

    let manager = ChannelManager::new(pool, channel, config.clone()).await;

    (config, manager)
}

async fn timed_stop(sec: u64, manager: ChannelManager) {
    tokio::time::sleep(tokio::time::Duration::from_secs(sec)).await;

    println!("Timed stop of process");

    manager.channel.lock().await.active = false;
    manager.stop_all(false).await;
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_missing() {
    let (mut config, manager) = prepare_config().await;

    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-07T23:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    if let Err(e) = player(manager.clone()).await {
        eprintln!("{e:?}");
    };

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-08");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_next_missing() {
    let (mut config, manager) = prepare_config().await;

    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-09T23:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    if let Err(e) = player(manager.clone()).await {
        eprintln!("{e:?}");
    };

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-10");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_to_short() {
    let (mut config, manager) = prepare_config().await;

    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    let manager_clone = manager.clone();

    set_mock_time(&Some("2024-01-31T05:59:40+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    if let Err(e) = player(manager.clone()).await {
        eprintln!("{e:?}");
    };

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2024-01-31");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_init_after_list_end() {
    let (mut config, manager) = prepare_config().await;

    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    let manager_clone = manager.clone();

    set_mock_time(&Some("2024-01-31T05:59:47+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    if let Err(e) = player(manager.clone()).await {
        eprintln!("{e:?}");
    };

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2024-01-31");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_change_at_midnight() {
    let (mut config, manager) = prepare_config().await;

    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-08T23:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    if let Err(e) = player(manager.clone()).await {
        eprintln!("{e:?}");
    };

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-09");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_change_before_midnight() {
    let (mut config, manager) = prepare_config().await;

    config.playlist.day_start = "23:59:45".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-08T23:59:30+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(35, manager_clone));

    if let Err(e) = player(manager.clone()).await {
        eprintln!("{e:?}");
    };

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-09");
}

#[tokio::test]
#[serial]
#[ignore]
async fn playlist_change_at_six() {
    let (mut config, manager) = prepare_config().await;

    config.playlist.day_start = "06:00:00".into();
    config.playlist.start_sec = Some(21600.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86400.0);

    manager.update_config(config).await;

    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    let manager_clone = manager.clone();

    set_mock_time(&Some("2023-02-09T05:59:45+01:00".to_string())).unwrap();

    tokio::spawn(timed_stop(28, manager_clone));

    if let Err(e) = player(manager.clone()).await {
        eprintln!("{e:?}");
    };

    let playlist_date = &*manager.current_date.lock().await;

    assert_eq!(playlist_date, "2023-02-09");
}
