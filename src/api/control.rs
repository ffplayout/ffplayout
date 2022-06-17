use reqwest::{
    header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE},
    Client, Response,
};
use serde::{Deserialize, Serialize};
use simplelog::*;

use crate::api::{
    errors::ServiceError, handles::db_get_settings, models::TextPreset, utils::read_playout_config,
};
use crate::utils::PlayoutConfig;

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RpcObj<T> {
    jsonrpc: String,
    id: i64,
    method: String,
    params: T,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct TextParams {
    control: String,
    message: TextPreset,
}

impl<T> RpcObj<T> {
    fn new(id: i64, method: String, params: T) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method,
            params,
        }
    }
}

fn create_header(auth: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        "Content-Type: application/json".parse().unwrap(),
    );
    headers.insert(AUTHORIZATION, auth.parse().unwrap());

    headers
}

async fn playout_config(channel_id: &i64) -> Result<PlayoutConfig, ServiceError> {
    if let Ok(settings) = db_get_settings(channel_id).await {
        if let Ok(config) = read_playout_config(&settings.config_path) {
            return Ok(config);
        }
    }

    Err(ServiceError::BadRequest(
        "Error in getting config!".to_string(),
    ))
}

pub async fn send_message(id: i64, message: TextPreset) -> Result<Response, ServiceError> {
    let config = playout_config(&id).await?;
    let url = format!("http://{}", config.rpc_server.address);
    let client = Client::new();
    let json_obj = RpcObj::new(
        id,
        "player".into(),
        TextParams {
            control: "text".into(),
            message,
        },
    );

    match client
        .post(&url)
        .headers(create_header(&config.rpc_server.authorization))
        .json(&json_obj)
        .send()
        .await
    {
        Ok(result) => Ok(result),
        Err(e) => {
            error!("{e:?}");
            Err(ServiceError::BadRequest(e.to_string()))
        }
    }
}
