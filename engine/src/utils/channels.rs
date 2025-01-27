use std::{path::PathBuf, sync::Arc};

use log::*;
use sqlx::{Pool, Sqlite};
use tokio::sync::Mutex;

use crate::db::{handles, models::Channel};
use crate::player::controller::{ChannelController, ChannelManager};
use crate::utils::{config::get_config, copy_assets, errors::ServiceError, mail::MailQueue};

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

pub async fn create_channel(
    conn: &Pool<Sqlite>,
    controllers: Arc<Mutex<ChannelController>>,
    queue: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    target_channel: Channel,
) -> Result<Channel, ServiceError> {
    const OUTPUT_PARM: &str = "-c:v libx264 -crf 23 -x264-params keyint=50:min-keyint=25:scenecut=-1 -maxrate 1300k -bufsize 2600k -preset faster -tune zerolatency -profile:v Main -level 3.1 -c:a aac -ar 44100 -b:a 128k -flags +cgop -f hls -hls_time 6 -hls_list_size 600 -hls_flags append_list+delete_segments+omit_endlist -hls_segment_filename live/stream-%d.ts live/stream.m3u8";

    let channel = handles::insert_channel(conn, target_channel).await?;

    handles::new_channel_presets(conn, channel.id).await?;
    handles::update_channel(conn, channel.id, channel.clone()).await?;
    handles::insert_advanced_configuration(conn, channel.id).await?;
    handles::insert_configuration(conn, channel.id, OUTPUT_PARM).await?;

    let config = get_config(conn, channel.id).await?;

    if let Err(e) = copy_assets(&PathBuf::from(&config.storage.path)).await {
        error!("{e}");
    };

    let m_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail.clone())));
    let manager = ChannelManager::new(conn.clone(), channel.clone(), config);

    controllers.lock().await.add(manager);
    queue.lock().await.push(m_queue);

    map_global_admins(conn).await?;

    Ok(channel)
}

pub async fn delete_channel(
    conn: &Pool<Sqlite>,
    id: i32,
    controllers: Arc<Mutex<ChannelController>>,
    queue: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> Result<(), ServiceError> {
    let channel = handles::select_channel(conn, &id).await?;
    handles::delete_channel(conn, &channel.id).await?;
    controllers.lock().await.remove(id).await;
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
