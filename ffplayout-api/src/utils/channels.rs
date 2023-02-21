use std::fs;

use simplelog::*;
use sqlx::{Pool, Sqlite};

use crate::utils::{
    control::{control_service, ServiceCmd},
    errors::ServiceError,
};

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

    fs::copy(
        "/usr/share/ffplayout/ffplayout.yml.orig",
        &target_channel.config_path,
    )?;

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
