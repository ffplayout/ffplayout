use std::fs;

use sqlx::sqlite::SqlitePoolOptions;

use ffplayout::db::handles;
use ffplayout::player::{controller::ChannelManager, utils::Media};
use ffplayout::utils::config::{OutputMode::*, PlayoutConfig};

async fn get_config() -> (PlayoutConfig, ChannelManager) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    sqlx::query(
        r#"
        UPDATE global SET public = "assets/hls", logs = "assets/log", playlists = "assets/playlists", storage = "assets/storage";
        UPDATE channels SET public = "assets/hls", playlists = "assets/playlists", storage = "assets/storage";
        UPDATE configurations SET processing_width = 1024, processing_height = 576, processing_volume = 0.05;
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let config = PlayoutConfig::new(&pool, 1).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(pool, channel, config.clone()).await;

    (config, manager)
}

#[tokio::test]
async fn simple_filtering() {
    let (mut config, _) = get_config().await;

    config.output.mode = Stream;
    config.processing.add_logo = true;
    let logo_path = fs::canonicalize("./assets/logo.png").unwrap();
    config.processing.logo_path = logo_path.to_string_lossy().to_string();

    let mut media = Media::new(0, "./assets/media_mix/with_audio.mp4", true).await;
    media.add_filter(&config, &None).await;

    let _f = media.filter.unwrap().cmd();

    // println!("{f:?}");
}
