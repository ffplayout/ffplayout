use std::{
    process,
    process::{Command, Stdio},
};

use simplelog::*;

use crate::utils::Config;

pub fn output(config: Config, log_format: String) -> process::Child {
    let mut enc_filter: Vec<String> = vec![];

    let mut enc_cmd = vec![
        "-hide_banner",
        "-nostats",
        "-v",
        log_format.as_str(),
        "-i",
        "pipe:0",
    ];

    if config.text.add_text && !config.text.over_pre {
        let text_filter: String = format!(
            "null,zmq=b=tcp\\\\://'{}',drawtext=text='':fontfile='{}'",
            config.text.bind_address.replace(":", "\\:"),
            config.text.fontfile
        );

        enc_filter = vec!["-vf".to_string(), text_filter];
    }

    enc_cmd.append(&mut enc_filter.iter().map(String::as_str).collect());

    debug!("Encoder CMD: <bright-blue>{:?}</>", enc_cmd);

    let enc_proc = match Command::new("ffplay")
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
