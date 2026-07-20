use std::sync::Arc;

use log::*;
use sqlx::{Pool, Sqlite};
use tokio::sync::{Mutex, RwLock};

use crate::{
    db::{
        handles,
        models::{self, Channel},
    },
    player::controller::{ChannelController, ChannelManager},
    utils::{config::get_config, errors::ServiceError, mail::MailQueue, system::SystemStat},
};

use super::config::OutputMode;

async fn map_global_admins(conn: &Pool<Sqlite>) -> Result<(), ServiceError> {
    sqlx::query(
        "INSERT OR IGNORE INTO user_channels (channel_id, user_id)
         SELECT channels.id, user.id FROM channels CROSS JOIN user WHERE user.role_id = 1",
    )
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn initialize_channels(
    conn: &Pool<Sqlite>,
    controllers: Arc<RwLock<ChannelController>>,
    queue: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    shutdown: tokio_util::sync::CancellationToken,
    system: SystemStat,
    copy_assets: bool,
) -> Result<(), ServiceError> {
    let channels = handles::select_related_channels(conn, None).await?;

    for (index, channel) in channels.into_iter().enumerate() {
        let config = get_config(conn, channel.id).await?;
        let mail_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail.clone())));
        let active = channel.active;
        let manager = ChannelManager::new(
            conn.clone(),
            channel,
            config,
            shutdown.clone(),
            system.clone(),
        )
        .await?;

        if copy_assets
            && index == 0
            && let Err(error) = manager.storage.copy_assets().await
        {
            warn!("Could not copy initial storage assets: {error}");
        }

        queue.lock().await.push(mail_queue);

        if active {
            manager.start().await?;
        }

        controllers.write().await.add(manager);
    }

    Ok(())
}

pub async fn create_channel(
    conn: &Pool<Sqlite>,
    controllers: Arc<RwLock<ChannelController>>,
    queue: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    shutdown: tokio_util::sync::CancellationToken,
    system: SystemStat,
    target_channel: Channel,
) -> Result<Channel, ServiceError> {
    let channel = create_channel_records(conn, target_channel).await?;

    let config = match get_config(conn, channel.id).await {
        Ok(config) => config,
        Err(error) => {
            rollback_channel_creation(conn, channel.id).await;
            return Err(error);
        }
    };

    let m_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail.clone())));
    let manager =
        match ChannelManager::new(conn.clone(), channel.clone(), config, shutdown, system).await {
            Ok(manager) => manager,
            Err(error) => {
                rollback_channel_creation(conn, channel.id).await;
                return Err(error);
            }
        };

    if let Err(e) = manager.storage.copy_assets().await {
        error!("{e}");
    };

    controllers.write().await.add(manager);
    queue.lock().await.push(m_queue);

    Ok(channel)
}

async fn rollback_channel_creation(conn: &Pool<Sqlite>, channel_id: i32) {
    if let Err(error) = handles::delete_channel(conn, &channel_id).await {
        error!("Could not roll back channel {channel_id} after initialization failed: {error}");
    }
}

async fn create_channel_records(
    conn: &Pool<Sqlite>,
    target_channel: Channel,
) -> Result<Channel, ServiceError> {
    let mut transaction = conn.begin().await?;
    let channel = handles::insert_channel(&mut *transaction, target_channel).await?;
    let outputs = [
        models::Output::new(channel.id, OutputMode::HLS),
        models::Output::new(channel.id, OutputMode::Stream),
        models::Output::new(channel.id, OutputMode::Desktop),
    ];

    handles::new_channel_presets(&mut *transaction, channel.id).await?;

    let mut output_id = 1;

    for (index, output) in outputs.iter().enumerate() {
        let id = handles::insert_output(&mut *transaction, channel.id, output).await?;

        if index == 0 {
            output_id = id;
        }
    }

    handles::insert_configuration(&mut *transaction, channel.id, output_id).await?;
    sqlx::query(
        "INSERT OR IGNORE INTO user_channels (channel_id, user_id)
         SELECT $1, id FROM user WHERE role_id = 1",
    )
    .bind(channel.id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(channel)
}

pub async fn delete_channel(
    conn: &Pool<Sqlite>,
    id: i32,
    controllers: Arc<RwLock<ChannelController>>,
    queue: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> Result<(), ServiceError> {
    let channel = handles::select_channel(conn, &id).await?;

    let manager = {
        let controller = controllers.read().await;
        controller.get(id)
    };
    if let Some(manager) = manager {
        manager.channel.lock().await.active = false;
        manager.stop_all(false).await;
        manager.stop_supervisor().await;
    }

    handles::delete_channel(conn, &channel.id).await?;
    controllers.write().await.remove(id);
    let mut queue_guard = queue.lock().await;
    let mut new_queue = Vec::with_capacity(queue_guard.len());

    for q in queue_guard.iter() {
        if q.lock().await.id != id {
            new_queue.push(q.clone());
        }
    }

    *queue_guard = new_queue;

    map_global_admins(conn).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    #[tokio::test]
    async fn channel_creation_rolls_back_all_records_on_failure() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        sqlx::query(
            "CREATE TRIGGER reject_test_configuration
             BEFORE INSERT ON configurations WHEN NEW.channel_id > 1
             BEGIN SELECT RAISE(FAIL, 'forced failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels")
            .fetch_one(&pool)
            .await
            .unwrap();

        let result = create_channel_records(
            &pool,
            Channel {
                name: "rollback-test".to_string(),
                ..Channel::default()
            },
        )
        .await;

        assert!(result.is_err());
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after, before);
        let orphan_outputs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM outputs WHERE channel_id NOT IN (SELECT id FROM channels)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphan_outputs, 0);
    }

    #[tokio::test]
    async fn channel_creation_assigns_the_channel_to_every_global_admin() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        handles::db_migrate(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO user (mail, username, password, role_id, two_factor) VALUES
             ('one@example.org', 'admin-one', 'hash', 1, 0),
             ('two@example.org', 'admin-two', 'hash', 1, 0),
             ('user@example.org', 'regular-user', 'hash', 3, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let channel = create_channel_records(
            &pool,
            Channel {
                name: "admin-mapping-test".to_string(),
                ..Channel::default()
            },
        )
        .await
        .unwrap();
        let assigned_roles: Vec<i32> = sqlx::query_scalar(
            "SELECT user.role_id FROM user_channels
             JOIN user ON user.id = user_channels.user_id
             WHERE user_channels.channel_id = $1 ORDER BY user.role_id",
        )
        .bind(channel.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(assigned_roles, [1, 1]);
    }
}
