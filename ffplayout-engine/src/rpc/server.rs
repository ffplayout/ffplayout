use std::{fmt, sync::atomic::Ordering};

use regex::Regex;
extern crate serde;
extern crate serde_json;
extern crate tiny_http;

use futures::executor::block_on;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};
use serde_json::{json, Map};
use simplelog::*;
use std::collections::HashMap;
use std::io::{Cursor, Error as IoError};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::rpc::zmq_send;
use crate::utils::{get_data_map, get_media_map};
use ffplayout_lib::utils::{
    get_delta, write_status, Ingest, OutputMode::*, PlayerControl, PlayoutConfig, PlayoutStatus,
    ProcessControl,
};

#[derive(Default, Deserialize, Clone, Debug)]
struct TextFilter {
    text: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    x: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    y: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    fontsize: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    line_spacing: Option<String>,
    fontcolor: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    alpha: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    r#box: Option<String>,
    boxcolor: Option<String>,
    #[serde(default, deserialize_with = "deserialize_number_or_string")]
    boxborderw: Option<String>,
}

/// Deserialize number or string
pub fn deserialize_number_or_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrNumberVisitor;

    impl<'de> Visitor<'de> for StringOrNumberVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a number")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            let re = Regex::new(r"0,([0-9]+)").unwrap();
            let clean_string = re.replace_all(value, "0.$1").to_string();
            Ok(Some(clean_string))
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }
    }

    deserializer.deserialize_any(StringOrNumberVisitor)
}

impl fmt::Display for TextFilter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let escaped_text = self
            .text
            .clone()
            .unwrap_or_default()
            .replace('\'', "'\\\\\\''")
            .replace('\\', "\\\\\\\\")
            .replace('%', "\\\\\\%")
            .replace(':', "\\:");

        let mut s = format!("text='{escaped_text}'");

        if let Some(v) = &self.x {
            if !v.is_empty() {
                s.push_str(&format!(":x='{v}'"));
            }
        }
        if let Some(v) = &self.y {
            if !v.is_empty() {
                s.push_str(&format!(":y='{v}'"));
            }
        }
        if let Some(v) = &self.fontsize {
            if !v.is_empty() {
                s.push_str(&format!(":fontsize={v}"));
            }
        }
        if let Some(v) = &self.line_spacing {
            if !v.is_empty() {
                s.push_str(&format!(":line_spacing={v}"));
            }
        }
        if let Some(v) = &self.fontcolor {
            if !v.is_empty() {
                s.push_str(&format!(":fontcolor={v}"));
            }
        }
        if let Some(v) = &self.alpha {
            if !v.is_empty() {
                s.push_str(&format!(":alpha='{v}'"));
            }
        }
        if let Some(v) = &self.r#box {
            if !v.is_empty() {
                s.push_str(&format!(":box={v}"));
            }
        }
        if let Some(v) = &self.boxcolor {
            if !v.is_empty() {
                s.push_str(&format!(":boxcolor={v}"));
            }
        }
        if let Some(v) = &self.boxborderw {
            if !v.is_empty() {
                s.push_str(&format!(":boxborderw={v}"));
            }
        }

        write!(f, "{s}")
    }
}

/// Covert JSON string to ffmpeg filter command.
fn filter_from_json(raw_text: serde_json::Value) -> String {
    let filter: TextFilter = serde_json::from_value(raw_text).unwrap_or_default();

    filter.to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseData {
    message: String,
}

/// Read the request body and convert it to a string
fn read_request_body(request: &mut Request) -> Result<String, IoError> {
    let mut buffer = String::new();
    let body = request.as_reader();

    match body.read_to_string(&mut buffer) {
        Ok(_) => Ok(buffer),
        Err(error) => Err(error),
    }
}

/// create client response in JSON format
fn json_response(data: serde_json::Map<String, serde_json::Value>) -> Response<Cursor<Vec<u8>>> {
    let response_body = serde_json::to_string(&data).unwrap();

    // create HTTP-Response
    Response::from_string(response_body)
        .with_status_code(200)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
}

/// create client error message
fn error_response(answer: &str, code: i32) -> Response<Cursor<Vec<u8>>> {
    error!("RPC: {answer}");

    Response::from_string(answer)
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/plain"[..]).unwrap())
}

/// control playout: jump to last clip
fn control_back(
    config: &PlayoutConfig,
    play_control: &PlayerControl,
    playout_stat: &PlayoutStatus,
    proc: &ProcessControl,
) -> Response<Cursor<Vec<u8>>> {
    let current_date = playout_stat.current_date.lock().unwrap().clone();
    let current_list = play_control.current_list.lock().unwrap();
    let mut date = playout_stat.date.lock().unwrap();
    let index = play_control.current_index.load(Ordering::SeqCst);
    let mut time_shift = playout_stat.time_shift.lock().unwrap();

    if index > 1 && current_list.len() > 1 {
        if let Some(proc) = proc.decoder_term.lock().unwrap().as_mut() {
            if let Err(e) = proc.kill() {
                error!("Decoder {e:?}")
            };

            if let Err(e) = proc.wait() {
                error!("Decoder {e:?}")
            };

            info!("Move to last clip");
            let mut data_map = Map::new();
            let mut media = current_list[index - 2].clone();
            play_control.current_index.fetch_sub(2, Ordering::SeqCst);

            if let Err(e) = media.add_probe(false) {
                error!("{e:?}");
            };

            let (delta, _) = get_delta(config, &media.begin.unwrap_or(0.0));
            *time_shift = delta;
            date.clone_from(&current_date);
            write_status(config, &current_date, delta);

            data_map.insert("operation".to_string(), json!("move_to_last"));
            data_map.insert("shifted_seconds".to_string(), json!(delta));
            data_map.insert("media".to_string(), get_media_map(media));

            return json_response(data_map);
        }

        return error_response("Jump to last clip failed!", 500);
    }
    error_response("Clip index out of range!", 400)
}

/// control playout: jump to next clip
fn control_next(
    config: &PlayoutConfig,
    play_control: &PlayerControl,
    playout_stat: &PlayoutStatus,
    proc: &ProcessControl,
) -> Response<Cursor<Vec<u8>>> {
    let current_date = playout_stat.current_date.lock().unwrap().clone();
    let current_list = play_control.current_list.lock().unwrap();
    let mut date = playout_stat.date.lock().unwrap();
    let index = play_control.current_index.load(Ordering::SeqCst);
    let mut time_shift = playout_stat.time_shift.lock().unwrap();

    if index < current_list.len() {
        if let Some(proc) = proc.decoder_term.lock().unwrap().as_mut() {
            if let Err(e) = proc.kill() {
                error!("Decoder {e:?}")
            };

            if let Err(e) = proc.wait() {
                error!("Decoder {e:?}")
            };

            info!("Move to next clip");

            let mut data_map = Map::new();
            let mut media = current_list[index].clone();

            if let Err(e) = media.add_probe(false) {
                error!("{e:?}");
            };

            let (delta, _) = get_delta(config, &media.begin.unwrap_or(0.0));
            *time_shift = delta;
            date.clone_from(&current_date);
            write_status(config, &current_date, delta);

            data_map.insert("operation".to_string(), json!("move_to_next"));
            data_map.insert("shifted_seconds".to_string(), json!(delta));
            data_map.insert("media".to_string(), get_media_map(media));

            return json_response(data_map);
        }

        return error_response("Jump to next clip failed!", 500);
    }

    error_response("Last clip can not be skipped!", 400)
}

/// control playout: reset playlist state
fn control_reset(
    config: &PlayoutConfig,
    playout_stat: &PlayoutStatus,
    proc: &ProcessControl,
) -> Response<Cursor<Vec<u8>>> {
    let current_date = playout_stat.current_date.lock().unwrap().clone();
    let mut date = playout_stat.date.lock().unwrap();
    let mut time_shift = playout_stat.time_shift.lock().unwrap();

    if let Some(proc) = proc.decoder_term.lock().unwrap().as_mut() {
        if let Err(e) = proc.kill() {
            error!("Decoder {e:?}")
        };

        if let Err(e) = proc.wait() {
            error!("Decoder {e:?}")
        };

        info!("Reset playout to original state");
        let mut data_map = Map::new();
        *time_shift = 0.0;
        date.clone_from(&current_date);
        playout_stat.list_init.store(true, Ordering::SeqCst);

        write_status(config, &current_date, 0.0);

        data_map.insert("operation".to_string(), json!("reset_playout_state"));

        return json_response(data_map);
    }

    error_response("Reset playout state failed!", 400)
}

/// control playout: stop playlout
fn control_stop(proc: &ProcessControl) -> Response<Cursor<Vec<u8>>> {
    proc.stop_all();

    let mut data_map = Map::new();
    data_map.insert("message".to_string(), json!("Stop playout!"));

    json_response(data_map)
}

/// control playout: create text filter for ffmpeg
fn control_text(
    data: HashMap<String, serde_json::Value>,
    config: &PlayoutConfig,
    playout_stat: &PlayoutStatus,
    proc: &ProcessControl,
) -> Response<Cursor<Vec<u8>>> {
    if data.contains_key("message") {
        let filter = filter_from_json(data["message"].clone());
        debug!("Got drawtext command: <bright-blue>\"{filter}\"</>");
        let mut data_map = Map::new();

        if !filter.is_empty() && config.text.zmq_stream_socket.is_some() {
            if let Some(clips_filter) = playout_stat.chain.clone() {
                *clips_filter.lock().unwrap() = vec![filter.clone()];
            }

            if config.out.mode == HLS {
                if proc.server_is_running.load(Ordering::SeqCst) {
                    let filter_server = format!("drawtext@dyntext reinit {filter}");

                    if let Ok(reply) = block_on(zmq_send(
                        &filter_server,
                        &config.text.zmq_server_socket.clone().unwrap(),
                    )) {
                        data_map.insert("message".to_string(), json!(reply));
                        return json_response(data_map);
                    };
                } else if let Err(e) = proc.stop(Ingest) {
                    error!("Ingest {e:?}")
                }
            }

            if config.out.mode != HLS || !proc.server_is_running.load(Ordering::SeqCst) {
                let filter_stream = format!("drawtext@dyntext reinit {filter}");

                if let Ok(reply) = block_on(zmq_send(
                    &filter_stream,
                    &config.text.zmq_stream_socket.clone().unwrap(),
                )) {
                    data_map.insert("message".to_string(), json!(reply));
                    return json_response(data_map);
                };
            }
        }
    }

    error_response("text message missing!", 400)
}

/// media info: get infos about current clip
fn media_current(
    config: &PlayoutConfig,
    playout_stat: &PlayoutStatus,
    play_control: &PlayerControl,
    proc: &ProcessControl,
) -> Response<Cursor<Vec<u8>>> {
    if let Some(media) = play_control.current_media.lock().unwrap().clone() {
        let data_map = get_data_map(
            config,
            media,
            playout_stat,
            proc.server_is_running.load(Ordering::SeqCst),
        );

        return json_response(data_map);
    };

    error_response("No current clip...", 204)
}

/// media info: get infos about next clip
fn media_next(
    config: &PlayoutConfig,
    playout_stat: &PlayoutStatus,
    play_control: &PlayerControl,
) -> Response<Cursor<Vec<u8>>> {
    let index = play_control.current_index.load(Ordering::SeqCst);
    let current_list = play_control.current_list.lock().unwrap();

    if index < current_list.len() {
        let media = current_list[index].clone();

        let data_map = get_data_map(config, media, playout_stat, false);

        return json_response(data_map);
    }

    error_response("There is no next clip", 500)
}

/// media info: get infos about last clip
fn media_last(
    config: &PlayoutConfig,
    playout_stat: &PlayoutStatus,
    play_control: &PlayerControl,
) -> Response<Cursor<Vec<u8>>> {
    let index = play_control.current_index.load(Ordering::SeqCst);
    let current_list = play_control.current_list.lock().unwrap();

    if index > 1 && index - 2 < current_list.len() {
        let media = current_list[index - 2].clone();

        let data_map = get_data_map(config, media, playout_stat, false);

        return json_response(data_map);
    }

    error_response("There is no last clip", 500)
}

/// response builder
/// convert request body to struct and create response according to the request values
fn build_response(
    mut request: Request,
    config: &PlayoutConfig,
    play_control: &PlayerControl,
    playout_stat: &PlayoutStatus,
    proc_control: &ProcessControl,
) {
    if let Ok(body) = read_request_body(&mut request) {
        if let Ok(data) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&body) {
            if let Some(control_value) = data.get("control").and_then(|c| c.as_str()) {
                match control_value {
                    "back" => {
                        let _ = request.respond(control_back(
                            config,
                            play_control,
                            playout_stat,
                            proc_control,
                        ));
                    }
                    "next" => {
                        let _ = request.respond(control_next(
                            config,
                            play_control,
                            playout_stat,
                            proc_control,
                        ));
                    }
                    "reset" => {
                        let _ = request.respond(control_reset(config, playout_stat, proc_control));
                    }
                    "stop_all" => {
                        let _ = request.respond(control_stop(proc_control));
                    }
                    "text" => {
                        let _ =
                            request.respond(control_text(data, config, playout_stat, proc_control));
                    }
                    _ => (),
                }
            } else if let Some(media_value) = data.get("media").and_then(|m| m.as_str()) {
                match media_value {
                    "current" => {
                        let _ = request.respond(media_current(
                            config,
                            playout_stat,
                            play_control,
                            proc_control,
                        ));
                    }
                    "next" => {
                        let _ = request.respond(media_next(config, playout_stat, play_control));
                    }
                    "last" => {
                        let _ = request.respond(media_last(config, playout_stat, play_control));
                    }
                    _ => (),
                }
            }
        } else {
            error!("Error parsing JSON request.");

            let _ = request.respond(error_response("Invalid JSON request", 400));
        }
    } else {
        error!("Error reading request body.");

        let _ = request.respond(error_response("Invalid JSON request", 500));
    }
}

/// request handler
/// check if authorization header with correct value exists and forward traffic to build_response()
fn handle_request(
    request: Request,
    config: &PlayoutConfig,
    play_control: &PlayerControl,
    playout_stat: &PlayoutStatus,
    proc_control: &ProcessControl,
) {
    // Check Authorization-Header
    match request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
    {
        Some(header) => {
            let auth_value = header.value.as_str();

            if auth_value == config.rpc_server.authorization {
                // create and send response
                build_response(request, config, play_control, playout_stat, proc_control)
            } else {
                let _ = request.respond(error_response("Unauthorized", 401));
            }
        }
        None => {
            let _ = request.respond(error_response("Missing authorization", 401));
        }
    }
}

/// JSON RPC Server
///
/// A simple rpc server for getting status information and controlling player:
///
/// - current clip information
/// - jump to next clip
/// - get last clip
/// - reset player state to original clip
pub fn run_server(
    config: PlayoutConfig,
    play_control: PlayerControl,
    playout_stat: PlayoutStatus,
    proc_control: ProcessControl,
) {
    let addr = config.rpc_server.address.clone();

    info!("RPC server listening on {addr}");

    let server = Server::http(addr).expect("Failed to start server");

    for request in server.incoming_requests() {
        match request.method() {
            Method::Post => handle_request(
                request,
                &config,
                &play_control,
                &playout_stat,
                &proc_control,
            ),
            _ => {
                // Method not allowed
                let response = Response::from_string("Method not allowed")
                    .with_status_code(405)
                    .with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/plain"[..]).unwrap(),
                    );

                let _ = request.respond(response);
            }
        }
    }
}
