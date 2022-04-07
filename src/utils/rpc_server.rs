use std::sync::{Arc, Mutex};

use jsonrpc_http_server::jsonrpc_core::{IoHandler, Params, Value};
use jsonrpc_http_server::{
    hyper, AccessControlAllowOrigin, DomainsValidation, Response, RestApi, ServerBuilder,
};
use process_control::Terminator;
use serde_json::{json, Map};
use simplelog::*;

use crate::utils::{
    get_delta, get_sec, sec_to_time, write_status, GlobalConfig, Media, PlayerControl,
    PlayoutStatus, ProcessControl,
};

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

fn kill_decoder(terminator: Arc<Mutex<Option<Terminator>>>) -> Result<(), String> {
    match &*terminator.lock().unwrap() {
        Some(decoder) => unsafe {
            if let Err(e) = decoder.terminate() {
                return Err(format!("Terminate decoder: {e}"));
            }
        },
        None => return Err("No decoder terminator found".to_string()),
    }

    Ok(())
}

pub async fn run_rpc(
    play_control: PlayerControl,
    playout_stat: PlayoutStatus,
    proc_control: ProcessControl,
) {
    let config = GlobalConfig::global();
    let mut io = IoHandler::default();
    let play = play_control.clone();
    let proc = proc_control.clone();

    io.add_sync_method("player", move |params: Params| {
        match params {
            Params::Map(map) => {
                if map.contains_key("control") && map["control"] == "next".to_string() {
                    if let Ok(_) = kill_decoder(proc.decoder_term.clone()) {
                        info!("Move to next clip");
                        let index = *play.index.lock().unwrap();

                        if index < play.current_list.lock().unwrap().len() {
                            let mut data_map = Map::new();
                            let mut media = play.current_list.lock().unwrap()[index].clone();
                            media.add_probe();

                            let (delta, _) = get_delta(&media.begin.unwrap_or(0.0));
                            *playout_stat.time_shift.lock().unwrap() = delta;
                            write_status(playout_stat.current_date.lock().unwrap().clone(), delta);

                            data_map.insert("operation".to_string(), json!("Move to next clip"));
                            data_map.insert("media".to_string(), get_media_map(media));

                            return Ok(Value::Object(data_map));
                        }
                    }
                    return Ok(Value::String("Move failed".to_string()));
                }

                if map.contains_key("control") && map["control"] == "back".to_string() {
                    if let Ok(_) = kill_decoder(proc.decoder_term.clone()) {
                        let index = *play.index.lock().unwrap();

                        if index > 1 && play.current_list.lock().unwrap().len() > 1 {
                            info!("Move to last clip");
                            let mut data_map = Map::new();
                            let mut media = play.current_list.lock().unwrap()[index - 2].clone();
                            *play.index.lock().unwrap() = index - 2;
                            media.add_probe();

                            let (delta, _) = get_delta(&media.begin.unwrap_or(0.0));
                            *playout_stat.time_shift.lock().unwrap() = delta;
                            write_status(playout_stat.current_date.lock().unwrap().clone(), delta);

                            data_map.insert("operation".to_string(), json!("Move to last clip"));
                            data_map.insert("media".to_string(), get_media_map(media));

                            return Ok(Value::Object(data_map));
                        }
                    }
                    return Ok(Value::String("Move failed".to_string()));
                }

                if map.contains_key("control") && map["control"] == "reset".to_string() {
                    *playout_stat.date.lock().unwrap() = String::new();
                    *playout_stat.time_shift.lock().unwrap() = 0.0;
                    *playout_stat.list_init.lock().unwrap() = true;

                    write_status(String::new().clone(), 0.0);

                    if let Err(e) = kill_decoder(proc.decoder_term.clone()) {
                        error!("{e}");
                    }

                    return Ok(Value::String("Reset playout to original state".to_string()));
                }

                if map.contains_key("media") && map["media"] == "current".to_string() {
                    if let Some(media) = play.current_media.lock().unwrap().clone() {
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
