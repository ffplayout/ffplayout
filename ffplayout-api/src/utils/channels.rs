use std::{fs, path::Path};

use rand::prelude::*;
use simplelog::*;
use sqlx::{Pool, Sqlite};

use crate::utils::{
    control::{control_service, ServiceCmd},
    errors::ServiceError,
};

use ffplayout_lib::utils::PlayoutConfig;

use crate::db::{handles, models::Channel};

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

    let mut config =
        PlayoutConfig::new(Some("/usr/share/ffplayout/ffplayout.yml.orig".to_string()));

    config.general.stat_file = format!(".ffp_{channel_name}",);

    config.logging.path = Path::new(&config.logging.path)
        .join(&channel_name)
        .to_string_lossy()
        .to_string();

    config.rpc_server.address = format!("127.0.0.1:70{:7>2}", channel_num);

    config.playlist.path = Path::new(&config.playlist.path)
        .join(channel_name)
        .to_string_lossy()
        .to_string();

    let file = fs::File::create(&target_channel.config_path)?;
    serde_yaml::to_writer(file, &config).unwrap();

    let new_channel = handles::insert_channel(conn, target_channel).await?;
    control_service(conn, new_channel.id, &ServiceCmd::Enable, None).await?;

    Ok(new_channel)
}

pub async fn delete_channel(conn: &Pool<Sqlite>, id: i32) -> Result<(), ServiceError> {
    let channel = handles::select_channel(conn, &id).await?;
    control_service(conn, channel.id, &ServiceCmd::Stop, None).await?;
    control_service(conn, channel.id, &ServiceCmd::Disable, None).await?;

    if let Err(e) = fs::remove_file(channel.config_path) {
        error!("{e}");
    };

    handles::delete_channel(conn, &id).await?;

    Ok(())
}
