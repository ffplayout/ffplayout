use serde_json::{Map, json};

use jsonrpc_http_server::jsonrpc_core::{IoHandler, Params, Value};
use jsonrpc_http_server::{
    hyper, AccessControlAllowOrigin, DomainsValidation, Response, RestApi, ServerBuilder,
};
use simplelog::*;

use crate::utils::{get_sec, sec_to_time, GlobalConfig, Media, ProcessControl};

fn get_media_map(media: Media) -> Value {
    json!({
        "seek": media.seek,
        "out": media.out,
        "duration": media.duration,
        "category": media.category,
        "source": media.source,
    })
}

fn get_data_map(config: &GlobalConfig, media: Media) -> Map<String, Value> {
    let mut data_map = Map::new();
    let begin = media.begin.unwrap_or(0.0);

    data_map.insert("play_mode".to_string(), json!(config.processing.mode));
    data_map.insert("index".to_string(), json!(media.index));
    data_map.insert("start_sec".to_string(), json!(begin));

    if begin > 0.0 {
        let played_time = get_sec() - begin;
        let remaining_time = media.out - played_time;

        data_map.insert("start_time".to_string(), json!(sec_to_time(begin)));
        data_map.insert("played_sec".to_string(), json!(played_time));
        data_map.insert("remaining_sec".to_string(), json!(remaining_time));
    }

    data_map.insert("current_media".to_string(), get_media_map(media));

    data_map
}

pub async fn run_rpc(proc_control: ProcessControl) {
    let config = GlobalConfig::global();
    let mut io = IoHandler::default();
    let proc = proc_control.clone();

    io.add_sync_method("player", move |params: Params| {
        match params {
            Params::Map(map) => {
                if map.contains_key("control") && map["control"] == "next".to_string() {
                    if let Some(decoder) = &*proc.decoder_term.lock().unwrap() {
                        unsafe {
                            if let Ok(_) = decoder.terminate() {
                                info!("Move to next clip");

                                if let Some(media) = proc.current_media.lock().unwrap().clone() {
                                    let mut data_map = Map::new();
                                    data_map.insert("operation".to_string(), json!("Move to next clip"));
                                    data_map.insert("media".to_string(), get_media_map(media));

                                    return Ok(Value::Object(data_map));
                                };

                                return Ok(Value::String(format!("Move failed")));
                            }
                        }
                    }
                }

                if map.contains_key("control") && map["control"] == "back".to_string() {
                    if let Some(decoder) = &*proc.decoder_term.lock().unwrap() {
                        let index = *proc.index.lock().unwrap();

                        if index > 1 && proc.current_list.lock().unwrap().len() > 1 {
                            info!("Move to last clip");
                            let mut data_map = Map::new();
                            let mut media = proc.current_list.lock().unwrap()[index - 2].clone();
                            *proc.index.lock().unwrap() = index - 2;
                            media.add_probe();
                            data_map.insert("operation".to_string(), json!("Move to last clip"));
                            data_map.insert("media".to_string(), get_media_map(media));

                            unsafe {
                                if let Ok(_) = decoder.terminate() {
                                    return Ok(Value::Object(data_map));
                                }
                            }
                        }

                        return Ok(Value::String(format!("Move failed")));
                    }
                }

                if map.contains_key("media") && map["media"] == "current".to_string() {
                    if let Some(media) = proc.current_media.lock().unwrap().clone() {
                        let data_map = get_data_map(config, media);

                        return Ok(Value::Object(data_map));
                    };
                }
            }
            _ => return Ok(Value::String(format!("Wrong parameters..."))),
        }

        Ok(Value::String(format!("no parameters set...")))
    });

    let server = ServerBuilder::new(io)
        .cors(DomainsValidation::AllowOnly(vec![
            AccessControlAllowOrigin::Null,
        ]))
        .request_middleware(|request: hyper::Request<hyper::Body>| {
            if request.headers().contains_key("authorization")
                && request.headers()["authorization"] == config.rpc_server.authorization
            {
                if request.uri() == "/status" {
                    println!("{:?}", request.headers().contains_key("authorization"));
                    Response::ok("Server running OK.").into()
                } else {
                    request.into()
                }
            } else {
                Response::bad_request("No authorization header or valid key found!").into()
            }
        })
        .rest_api(RestApi::Secure)
        .start_http(&config.rpc_server.address.parse().unwrap())
        .expect("Unable to start RPC server");

    *proc_control.rpc_handle.lock().unwrap() = Some(server.close_handle().clone());

    server.wait();
}
