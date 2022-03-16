use std::{
    process,
    process::{Command, Stdio},
};

use simplelog::*;

use crate::utils::{GlobalConfig, Media};
use crate::filter::v_drawtext;

pub fn output(log_format: String) -> process::Child {
    let config = GlobalConfig::global();
    let mut enc_filter: Vec<String> = vec![];
    let mut preview: Vec<&str> = vec![];

    let mut enc_cmd = vec![
        "-hide_banner",
        "-nostats",
        "-v",
        log_format.as_str(),
        "-re",
        "-i",
        "pipe:0",
    ];

    if config.text.add_text && !config.text.over_pre {
        info!(
            "Using drawtext filter, listening on address: <yellow>{}</>",
            config.text.bind_address
        );

        let mut filter: String = "[0:v]null,".to_string();
        filter.push_str(v_drawtext::filter_node(&mut Media::new(0, "".to_string())).as_str());

        if config.out.preview {
            filter.push_str(",split=2[v_out1][v_out2]");

            preview = vec!["-map", "[v_out1]", "-map", "0:a"];
            preview.append(&mut config.out.preview_param.iter().map(String::as_str).collect());
            preview.append(&mut vec!["-map", "[v_out2]", "-map", "0:a"]);
        }

        enc_filter = vec!["-filter_complex".to_string(), filter];
    } else if config.out.preview {
        preview = config.out.preview_param.iter().map(String::as_str).collect()
    }

    enc_cmd.append(&mut enc_filter.iter().map(String::as_str).collect());
    enc_cmd.append(&mut preview);
    enc_cmd.append(&mut config.out.stream_param.iter().map(String::as_str).collect());

    debug!("Encoder CMD: <bright-blue>{:?}</>", enc_cmd);

    let enc_proc = match Command::new("ffmpeg")
        .args(enc_cmd)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            error!("couldn't spawn encoder process: {}", e);
            panic!("couldn't spawn encoder process: {}", e)
        }
        Ok(proc) => proc,
    };

    enc_proc
}
