use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use tokio::runtime::Runtime;

#[cfg(test)]
use chrono::prelude::*;

#[cfg(test)]
use ffplayout::db::handles;
use ffplayout::player::utils::*;
use ffplayout::utils::config::PlayoutConfig;
use ffplayout::utils::config::ProcessMode::Playlist;

async fn memory_db() -> Pool<Sqlite> {
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

    pool
}

fn get_pool() -> Pool<Sqlite> {
    Runtime::new().unwrap().block_on(memory_db())
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
    let pool = get_pool();
    let mut config = Runtime::new()
        .unwrap()
        .block_on(PlayoutConfig::new(&pool, 1));

    config.mail.recipient = "".into();
    config.processing.mode = Playlist;
    config.playlist.day_start = "00:00:00".into();
    config.playlist.length = "24:00:00".into();

    mock_time::set_mock_time("2022-05-09T23:59:59");
    let (delta, _) = get_delta(&config, &86401.0);

    assert!(delta < 2.0);
}
