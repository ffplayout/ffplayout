use std::{fs, path::PathBuf};

use rand::prelude::*;
use simplelog::*;
use sqlx::{Pool, Sqlite};

use crate::utils::{
    control::{control_service, ServiceCmd},
    errors::ServiceError,
};

use ffplayout_lib::utils::PlayoutConfig;

use crate::db::{handles, models::Channel};
use crate::utils::playout_config;

pub async fn create_channel(
    conn: &Pool<Sqlite>,
    target_channel: Channel,
) -> Result<Channel, ServiceError> {
    if !target_channel.service.starts_with("ffplayout@") {
        return Err(ServiceError::BadRequest("Bad service name!".to_string()));
    }

    if !target_channel.config_path.starts_with("/etc/ffplayout") {
        return Err(ServiceError::BadRequest("Bad config path!".to_string()));
    }

    let channel_name = target_channel.name.to_lowercase().replace(' ', "");
    let channel_num = match handles::select_last_channel(conn).await {
        Ok(num) => num + 1,
        Err(_) => rand::thread_rng().gen_range(71..99),
    };

    let mut config = PlayoutConfig::new(
        Some(PathBuf::from("/usr/share/ffplayout/ffplayout.toml.orig")),
        None,
    );

    config.general.stat_file = format!(".ffp_{channel_name}",);
    config.logging.path = config.logging.path.join(&channel_name);
    config.rpc_server.address = format!("127.0.0.1:70{:7>2}", channel_num);
    config.playlist.path = config.playlist.path.join(channel_name);

    config.out.output_param = config
        .out
        .output_param
        .replace("stream.m3u8", &format!("stream{channel_num}.m3u8"))
        .replace("stream-%d.ts", &format!("stream{channel_num}-%d.ts"));

    let toml_string = toml_edit::ser::to_string(&config)?;
    fs::write(&target_channel.config_path, toml_string)?;

    let new_channel = handles::insert_channel(conn, target_channel).await?;
    control_service(conn, &config, new_channel.id, &ServiceCmd::Enable, None).await?;

    Ok(new_channel)
}

pub async fn delete_channel(conn: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
    let channel = handles::select_channel(conn, &id).await?;
    let (config, _) = playout_config(conn, &id).await?;

    control_service(conn, &config, channel.id, &ServiceCmd::Stop, None).await?;
    control_service(conn, &config, channel.id, &ServiceCmd::Disable, None).await?;

    if let Err(e) = fs::remove_file(channel.config_path) {
        error!("{e}");
    };

    handles::delete_channel(conn, &id).await?;

    Ok(())
}
