use sqlx::sqlite::SqlitePoolOptions;
use tokio::runtime::Runtime;

#[cfg(test)]
use chrono::prelude::*;

#[cfg(test)]
use ffplayout::db::handles;
use ffplayout::player::{controller::ChannelManager, utils::*};
use ffplayout::utils::config::{PlayoutConfig, ProcessMode::Playlist};

async fn prepare_config() -> (PlayoutConfig, ChannelManager) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    sqlx::query(
        r#"
        UPDATE global SET hls_path = "assets/hls", logging_path = "assets/log",
            playlist_path = "assets/playlists", storage_path = "assets/storage";
        UPDATE configurations SET processing_width = 1024, processing_height = 576;
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let config = PlayoutConfig::new(&pool, 1).await;
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(Some(pool), channel, config.clone());

    (config, manager)
}

fn get_config() -> (PlayoutConfig, ChannelManager) {
    Runtime::new().unwrap().block_on(prepare_config())
}

#[test]
fn mock_date_time() {
    let time_str = "2022-05-20T06:00:00";
    let date_obj = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S");
    let time = Local.from_local_datetime(&date_obj.unwrap()).unwrap();

    mock_time::set_mock_time(time_str);

    assert_eq!(
        time.format("%Y-%m-%dT%H:%M:%S.2f").to_string(),
        time_now().format("%Y-%m-%dT%H:%M:%S.2f").to_string()
    );
}

#[test]
fn get_date_yesterday() {
    mock_time::set_mock_time("2022-05-20T05:59:24");

    let date = get_date(true, 21600.0, false);

    assert_eq!("2022-05-19".to_string(), date);
}

#[test]
fn get_date_tomorrow() {
    mock_time::set_mock_time("2022-05-20T23:59:58");

    let date = get_date(false, 0.0, true);

    assert_eq!("2022-05-21".to_string(), date);
}

#[test]
fn test_delta() {
    let (mut config, _) = get_config();

    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.length = "24:00:00".into();

    mock_time::set_mock_time("2022-05-09T23:59:59");
    let (delta, _) = get_delta(&config, &86401.0);

    assert!(delta < 2.0);
}
