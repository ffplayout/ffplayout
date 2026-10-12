use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tokio_util::sync::CancellationToken;

use crate::{
    db::handles,
    player::controller::ChannelManager,
    utils::{config::PlayoutConfig, system::SystemStat},
};

pub(crate) async fn channel_manager() -> (ChannelManager, SqlitePool) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();
    let storage = std::env::temp_dir().join(format!("ffplayout-manager-{}", uuid::Uuid::new_v4()));
    let playlists = storage.join("playlists");
    let logs = storage.join("logs");
    let public = storage.join("public");

    // Configuration loading creates playlists and logs too. Keep every path
    // inside the fixture root so tests need no system directory permissions.
    sqlx::query("UPDATE channels SET storage = $1, playlists = $2, public = $3")
        .bind(storage.to_string_lossy())
        .bind(playlists.to_string_lossy())
        .bind(public.to_string_lossy())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE config_global SET storage = $1, playlists = $2, logs = $3, public = $4")
        .bind(storage.to_string_lossy())
        .bind(playlists.to_string_lossy())
        .bind(logs.to_string_lossy())
        .bind(public.to_string_lossy())
        .execute(&pool)
        .await
        .unwrap();
    let config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(
        pool.clone(),
        channel,
        config,
        CancellationToken::new(),
        SystemStat::new(),
    )
    .await
    .unwrap();

    (manager, pool)
}
