use std::{ffi::OsStr, path::Path, sync::Arc};

use regex::Regex;
use tokio::sync::Mutex;

use crate::player::{
    controller::ProcessUnit::*,
    utils::{Media, custom_format},
};
use crate::utils::config::PlayoutConfig;

pub async fn filter_node(
    config: &PlayoutConfig,
    node: Option<&Media>,
    filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
) -> String {
    let mut filter = String::new();
    let font = if Path::new(&config.text.font_path).is_file() {
        format!(":fontfile='{}'", config.text.font_path)
    } else {
        String::new()
    };

    let zmq_socket = match node.map(|n| n.unit) {
        Some(Ingest) => config.text.zmq_server_socket.clone(),
        _ => config.text.zmq_stream_socket.clone(),
    };

    if config.text.text_from_filename && node.is_some() {
        let source = node.map_or("", |n| &n.source);
        let text = match Regex::new(&config.text.regex)
            .ok()
            .and_then(|r| r.captures(source))
        {
            Some(t) => t[1].to_string(),
            None => Path::new(&source)
                .file_stem()
                .unwrap_or_else(|| OsStr::new(&source))
                .to_string_lossy()
                .to_string(),
        };

        let escaped_text = text
            .replace('\'', "'\\\\\\''")
            .replace('%', "\\\\\\%")
            .replace(':', "\\:");

        filter = match &config.advanced.filter.drawtext_from_file {
            Some(drawtext) => custom_format(drawtext, &[&escaped_text, &config.text.style, &font]),
            None => format!("drawtext=text='{escaped_text}':{}{font}", config.text.style),
        };
    } else if let Some(socket) = zmq_socket {
        let mut filter_cmd = format!("text=''{font}");

        if let Some(chain) = filter_chain {
            if let Some(link) = chain.lock().await.iter().find(|&l| l.contains("text")) {
                filter_cmd = link.to_string();
            }
        }

        filter = match config.advanced.filter.drawtext_from_zmq.clone() {
            Some(drawtext) => custom_format(&drawtext, &[&socket.replace(':', "\\:"), &filter_cmd]),
            None => format!(
                "zmq=b=tcp\\\\://'{}',drawtext@dyntext={filter_cmd}",
                socket.replace(':', "\\:")
            ),
        };
    }

    filter
}
