use std::{
    io,
    sync::{Arc, Mutex},
};

use log::*;
use sqlx::{Pool, Sqlite};

use super::logging::MailQueue;
use crate::db::{handles, models::Channel};
use crate::player::controller::{ChannelController, ChannelManager};
use crate::utils::{config::PlayoutConfig, errors::ServiceError};

pub async fn create_channel(
    conn: &Pool<Sqlite>,
    controllers: Arc<Mutex<ChannelController>>,
    queue: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    target_channel: Channel,
) -> Result<Channel, ServiceError> {
    let channel = handles::insert_channel(conn, target_channel).await?;

    let output_param = format!("-c:v libx264 -crf 23 -x264-params keyint=50:min-keyint=25:scenecut=-1 -maxrate 1300k -bufsize 2600k -preset faster -tune zerolatency -profile:v Main -level 3.1 -c:a aac -ar 44100 -b:a 128k -flags +cgop -f hls -hls_time 6 -hls_list_size 600 -hls_flags append_list+delete_segments+omit_endlist -hls_segment_filename /usr/share/ffplayout/public/live/stream{0}-%d.ts /usr/share/ffplayout/public/live/stream{0}.m3u8", channel.id);

    handles::insert_advanced_configuration(conn, channel.id).await?;
    handles::insert_configuration(conn, channel.id, output_param).await?;

    let config = PlayoutConfig::new(conn, channel.id).await;
    let m_queue = Arc::new(Mutex::new(MailQueue::new(channel.id, config.mail.clone())));
    let manager = ChannelManager::new(Some(conn.clone()), channel.clone(), config);

    controllers
        .lock()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
        .add(manager);

    if let Ok(mut mqs) = queue.lock() {
        mqs.push(m_queue.clone());
    }

    if let Ok(channels) = handles::select_related_channels(conn, None).await {
        if let Ok(admins) = handles::select_global_admins(conn).await {
            for admin in admins {
                if let Err(e) = handles::update_user_channel(
                    conn,
                    admin.id,
                    channels
                        .iter()
                        .map(|c| c.id.to_string())
                        .collect::<Vec<String>>()
                        .join(","),
                )
                .await
                {
                    error!("Update global admin: {e}");
                };
            }
        }
    }

    Ok(channel)
}

pub async fn delete_channel(conn: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
    let channel = handles::select_channel(conn, &id).await?;
    // TODO: Remove Channel controller

    handles::delete_channel(conn, &channel.id).await?;

    Ok(())
}
