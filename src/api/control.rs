use log::error;
use reqwest::{
    header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE},
    Client, Response,
};
use serde::{Deserialize, Serialize};

use crate::api::{
    errors::ServiceError, handles::db_get_settings, models::TextPreset, utils::read_playout_config,
};

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

fn create_header(content: &str, auth: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, content.parse().unwrap());
    headers.insert(AUTHORIZATION, auth.parse().unwrap());

    headers
}

pub async fn send_message(id: i64, msg: TextPreset) -> Result<Response, ServiceError> {
    let client = Client::new();

    if let Ok(settings) = db_get_settings(&id).await {
        if let Ok(config) = read_playout_config(&settings.config_path) {
            let url = format!("http://{}", config.rpc_server.address);
            let json_obj = RpcObj::new(
                id,
                "player".into(),
                TextParams {
                    control: "text".into(),
                    message: msg,
                },
            );

            match client
                .post(&url)
                .headers(create_header(
                    "Content-Type: application/json",
                    &config.rpc_server.authorization,
                ))
                .json(&json_obj)
                .send()
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) => {
                    error!("{e:?}");
                    return Err(ServiceError::BadRequest(e.to_string()));
                }
            };
        }
    };

    Err(ServiceError::InternalServerError)
}
