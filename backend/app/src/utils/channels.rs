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
    let channels = handles::select_related_channels(conn, None).await?;
    let admins = handles::select_global_admins(conn).await?;

    for admin in admins {
        if let Err(e) =
            handles::insert_user_channel(conn, admin.id, channels.iter().map(|c| c.id).collect())
                .await
        {
            error!("Update global admin: {e}");
        };
    }

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
        .await;

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
    let channel = handles::insert_channel(conn, target_channel).await?;
    let outputs = [
        models::Output::new(channel.id, OutputMode::HLS),
        models::Output::new(channel.id, OutputMode::Stream),
        models::Output::new(channel.id, OutputMode::Desktop),
    ];

    handles::new_channel_presets(conn, channel.id).await?;
    handles::update_channel(conn, channel.id, channel.clone()).await?;

    let mut output_id = 1;

    for (index, output) in outputs.iter().enumerate() {
        let id = handles::insert_output(conn, channel.id, output).await?;

        if index == 0 {
            output_id = id;
        }
    }

    handles::insert_configuration(conn, channel.id, output_id).await?;

    let config = get_config(conn, channel.id).await?;

    let m_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail.clone())));
    let manager =
        ChannelManager::new(conn.clone(), channel.clone(), config, shutdown, system).await;

    if let Err(e) = manager.storage.copy_assets().await {
        error!("{e}");
    };

    controllers.write().await.add(manager);
    queue.lock().await.push(m_queue);

    map_global_admins(conn).await?;

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
