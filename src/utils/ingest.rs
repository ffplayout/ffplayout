use std::path::Path;

use simplelog::*;

use crate::utils::GlobalConfig;

fn overlay(config: &GlobalConfig) -> String {
    let mut logo_chain = String::new();

    if config.processing.add_logo && Path::new(&config.processing.logo).is_file() {
        let opacity = format!(
            "format=rgba,colorchannelmixer=aa={}",
            config.processing.logo_opacity
        );
        let logo_loop = "loop=loop=-1:size=1:start=0";
        logo_chain = format!("[v];movie={},{logo_loop},{opacity}", config.processing.logo);

        logo_chain
            .push_str(format!("[l];[v][l]{}:shortest=1", config.processing.logo_filter).as_str());
    }

    logo_chain
}

pub fn ingest_server(log_format: String) {
    let config = GlobalConfig::global();
    let mut filter = format!(
        "[0:v]fps={},scale={}:{},'setdar=dar={}",
        config.processing.fps,
        config.processing.width,
        config.processing.height,
        config.processing.aspect
    );

    filter.push_str(&overlay(&config));
    filter.push_str("[vout1]");
    let mut filter_list = vec!["-filter_complex", &filter, "-map", "[vout1]", "-map", "0:a"];

    let mut server_cmd = vec!["-hide_banner", "-nostats", "-v", log_format.as_str()];
    let stream_input = config.ingest.stream_input.clone();
    let stream_settings = config.processing.settings.clone().unwrap();

    server_cmd.append(&mut stream_input.iter().map(String::as_str).collect());
    server_cmd.append(&mut filter_list);
    server_cmd.append(&mut stream_settings.iter().map(String::as_str).collect());

    info!(
        "Start ingest server, listening on: <b><magenta>{}</></b>",
        stream_input.last().unwrap()
    );
}
