use std::collections::HashMap;

use reqwest::{
    header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE},
    Client, Response,
};
use serde::{Deserialize, Serialize};
use simplelog::*;

use crate::api::{errors::ServiceError, utils::playout_config};

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
    message: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ControlParams {
    control: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MediaParams {
    media: String,
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

async fn post_request<T>(id: i64, obj: RpcObj<T>) -> Result<Response, ServiceError>
where
    T: Serialize,
{
    let (config, _) = playout_config(&id).await?;
    let url = format!("http://{}", config.rpc_server.address);
    let client = Client::new();

    match client
        .post(&url)
        .headers(create_header(&config.rpc_server.authorization))
        .json(&obj)
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

pub async fn send_message(
    id: i64,
    message: HashMap<String, String>,
) -> Result<Response, ServiceError> {
    let json_obj = RpcObj::new(
        id,
        "player".into(),
        TextParams {
            control: "text".into(),
            message,
        },
    );

    post_request(id, json_obj).await
}

pub async fn control_state(id: i64, command: String) -> Result<Response, ServiceError> {
    let json_obj = RpcObj::new(id, "player".into(), ControlParams { control: command });

    post_request(id, json_obj).await
}

pub async fn media_info(id: i64, command: String) -> Result<Response, ServiceError> {
    let json_obj = RpcObj::new(id, "player".into(), MediaParams { media: command });

    post_request(id, json_obj).await
}
