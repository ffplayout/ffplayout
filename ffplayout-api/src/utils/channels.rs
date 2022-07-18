use std::fs;

use simplelog::*;

use crate::utils::{
    control::control_service,
    errors::ServiceError,
    handles::{db_add_channel, db_delete_channel, db_get_channel},
    models::Channel,
};

pub async fn create_channel(target_channel: Channel) -> Result<Channel, ServiceError> {
    if !target_channel.service.starts_with("ffplayout@") {
        return Err(ServiceError::BadRequest("Bad service name!".to_string()));
    }

    if !target_channel.config_path.starts_with("/etc/ffplayout") {
        return Err(ServiceError::BadRequest("Bad config path!".to_string()));
    }

    if let Ok(source_channel) = db_get_channel(&1).await {
        if fs::copy(&source_channel.config_path, &target_channel.config_path).is_ok() {
            match db_add_channel(target_channel).await {
                Ok(c) => {
                    if let Err(e) = control_service(c.id, "enable").await {
                        return Err(e);
                    }
                    return Ok(c);
                }
                Err(e) => {
                    return Err(ServiceError::Conflict(e.to_string()));
                }
            };
        }
    }

    Err(ServiceError::InternalServerError)
}

pub async fn delete_channel(id: i64) -> Result<(), ServiceError> {
    if let Ok(channel) = db_get_channel(&id).await {
        if control_service(channel.id, "stop").await.is_ok()
            && control_service(channel.id, "disable").await.is_ok()
        {
            if let Err(e) = fs::remove_file(channel.config_path) {
                error!("{e}");
            };
            match db_delete_channel(&id).await {
                Ok(_) => return Ok(()),
                Err(e) => return Err(ServiceError::Conflict(e.to_string())),
            }
        }
    }

    Err(ServiceError::InternalServerError)
}
