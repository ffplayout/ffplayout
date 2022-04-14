use std::{
    process,
    process::{Command, Stdio},
};

use simplelog::*;

use crate::filter::v_drawtext;
use crate::utils::{GlobalConfig, Media};

pub fn output(log_format: &str) -> process::Child {
    let config = GlobalConfig::global();

    let mut enc_filter: Vec<String> = vec![];

    let mut enc_cmd = vec![
        "-hide_banner",
        "-nostats",
        "-v",
        log_format,
        "-i",
        "pipe:0",
    ];

    if config.text.add_text && !config.text.over_pre {
        info!(
            "Using drawtext filter, listening on address: <yellow>{}</>",
            config.text.bind_address
        );

        let mut filter: String = "null,".to_string();
        filter.push_str(v_drawtext::filter_node(&mut Media::new(0, String::new(), false)).as_str());
        enc_filter = vec!["-vf".to_string(), filter];
    }

    enc_cmd.append(&mut enc_filter.iter().map(String::as_str).collect());

    debug!("Encoder CMD: <bright-blue>\"ffplay {}\"</>", enc_cmd.join(" "));

    let enc_proc = match Command::new("ffplay")
        .args(enc_cmd)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            error!("couldn't spawn encoder process: {e}");
            panic!("couldn't spawn encoder process: {e}")
        }
        Ok(proc) => proc,
    };

    enc_proc
}
