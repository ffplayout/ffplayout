use serde_json::{Map, Number};

use jsonrpc_http_server::jsonrpc_core::{IoHandler, Params, Value};
use jsonrpc_http_server::{
    hyper, AccessControlAllowOrigin, DomainsValidation, Response, RestApi, ServerBuilder,
};
use simplelog::*;

use crate::utils::{GlobalConfig, ProcessControl};

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
                                info!("Skip current clip");
                                return Ok(Value::String(format!("Skip current clip")));
                            }
                        }
                    }
                }

                if map.contains_key("media") && map["media"] == "current".to_string() {
                    if let Some(media) = proc.current_media.lock().unwrap().clone() {
                        let mut media_map = Map::new();
                        media_map.insert(
                            "begin".to_string(),
                            Value::Number(Number::from_f64(media.begin.unwrap_or(0.0)).unwrap()),
                        );
                        media_map.insert("source".to_string(), Value::String(media.source));

                        return Ok(Value::Object(media_map));
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
