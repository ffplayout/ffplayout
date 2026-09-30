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
    sqlx::query("UPDATE channels SET storage = $1; UPDATE config_global SET storage = $1")
        .bind(storage.to_string_lossy())
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
