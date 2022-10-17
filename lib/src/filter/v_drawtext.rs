use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use regex::Regex;

use crate::utils::{controller::ProcessUnit::*, Media, PlayoutConfig};

pub fn filter_node(
    config: &PlayoutConfig,
    node: Option<&Media>,
    filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
) -> String {
    let mut filter = String::new();
    let mut font = String::new();

    if Path::new(&config.text.fontfile).is_file() {
        font = format!(":fontfile='{}'", config.text.fontfile)
    }

    let zmq_socket = match node.map(|n| n.unit) {
        Some(Ingest) => config.text.zmq_server_socket.clone(),
        _ => config.text.zmq_stream_socket.clone(),
    };

    // TODO: in Rust 1.65 use let_chains instead
    if config.text.text_from_filename && node.is_some() {
        let source = node.unwrap_or(&Media::new(0, "", false)).source.clone();
        let regex: Regex = Regex::new(&config.text.regex).unwrap();

        let text: String = match regex.captures(&source) {
            Some(t) => t[1].to_string(),
            None => source,
        };

        let escape = text
            .replace('\'', "'\\\\\\''")
            .replace('%', "\\\\\\%")
            .replace(':', "\\:");
        filter = format!("drawtext=text='{escape}':{}{font}", config.text.style)
    } else if let Some(socket) = zmq_socket {
        let mut filter_cmd = format!("text=''{font}");

        if let Some(chain) = filter_chain {
            if let Some(link) = chain.lock().unwrap().iter().find(|&l| l.contains("text")) {
                filter_cmd = link.to_string();
            }
        }

        filter = format!(
            "zmq=b=tcp\\\\://'{}',drawtext@dyntext={filter_cmd}",
            socket.replace(':', "\\:")
        )
    }

    filter
}
