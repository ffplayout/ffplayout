use std::fs;

use rand::prelude::*;
use simplelog::*;
use sqlx::{Pool, Sqlite};

use crate::db::{handles, models::Channel};
use crate::utils::{config::PlayoutConfig, errors::ServiceError, playout_config};

pub async fn create_channel(
    conn: &Pool<Sqlite>,
    target_channel: Channel,
) -> Result<Channel, ServiceError> {
    let channel_name = target_channel.name.to_lowercase().replace(' ', "");
    let channel_num = match handles::select_last_channel(conn).await {
        Ok(num) => num + 1,
        Err(_) => rand::thread_rng().gen_range(71..99),
    };

    let mut config = PlayoutConfig::new(conn, channel_num).await;

    config.playlist.path = config.playlist.path.join(channel_name);

    config.output.output_param = config
        .output
        .output_param
        .replace("stream.m3u8", &format!("stream{channel_num}.m3u8"))
        .replace("stream-%d.ts", &format!("stream{channel_num}-%d.ts"));

    let new_channel = handles::insert_channel(conn, target_channel).await?;
    // TODO: Create Channel controller

    Ok(new_channel)
}

pub async fn delete_channel(conn: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
    let _channel = handles::select_channel(conn, &id).await?;
    let (_config, _) = playout_config(conn, &id).await?;

    // TODO: Remove Channel controller

    handles::delete_channel(conn, &id).await?;

    Ok(())
}
