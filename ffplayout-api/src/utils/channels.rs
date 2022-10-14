use std::fs;

use simplelog::*;

use crate::utils::{control::control_service, errors::ServiceError};

use crate::db::{handles, models::Channel};

pub async fn create_channel(target_channel: Channel) -> Result<Channel, ServiceError> {
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

    let new_channel = handles::insert_channel(target_channel).await?;
    control_service(new_channel.id, "enable").await?;

    Ok(new_channel)
}

pub async fn delete_channel(id: i32) -> Result<(), ServiceError> {
    let channel = handles::select_channel(&id).await?;
    control_service(channel.id, "stop").await?;
    control_service(channel.id, "disable").await?;

    if let Err(e) = fs::remove_file(channel.config_path) {
        error!("{e}");
    };

    handles::delete_channel(&id).await?;

    Ok(())
}
