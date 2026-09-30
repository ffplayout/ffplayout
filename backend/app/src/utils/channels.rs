use std::sync::Arc;

use log::*;
use sqlx::{Pool, Sqlite};
use tokio::sync::{Mutex, RwLock};

use crate::{
    db::{handles, models::Channel},
    player::controller::{ChannelController, ChannelManager},
    utils::{config::get_config, errors::ServiceError, mail::MailQueue, system::SystemStat},
};

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
        let mail_queue = Arc::new(Mutex::new(MailQueue::new(
            channel.id,
            config.mail.clone(),
            config.notification.clone(),
        )));
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
    let channel = handles::create_channel_records(conn, target_channel).await?;

    let config = match get_config(conn, channel.id).await {
        Ok(config) => config,
        Err(error) => {
            rollback_channel_creation(conn, channel.id).await;
            return Err(error);
        }
    };

    let m_queue = Arc::new(Mutex::new(MailQueue::new(
        channel.id,
        config.mail.clone(),
        config.notification.clone(),
    )));
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

    handles::map_global_admins(conn).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use crate::test_support::channel_manager;

    use super::*;

    #[tokio::test]
    async fn deleting_channel_stops_and_removes_manager() {
        let (manager, pool) = channel_manager().await;
        manager.is_alive.store(true, Ordering::SeqCst);
        let controller = Arc::new(RwLock::new(ChannelController::new()));
        controller.write().await.add(manager.clone());
        let mail_queues = Arc::new(Mutex::new(Vec::new()));

        delete_channel(&pool, manager.id, controller.clone(), mail_queues)
            .await
            .unwrap();

        assert!(!manager.is_alive.load(Ordering::SeqCst));
        assert!(controller.read().await.get(manager.id).is_none());
        assert!(handles::select_channel(&pool, &manager.id).await.is_err());
        tokio::fs::remove_dir_all(manager.storage.root.read().await.clone())
            .await
            .unwrap();
    }
}
