use std::process::{self, Command, Stdio};

use simplelog::*;

use crate::utils::prepare_output_cmd;
use ffplayout_lib::{
    utils::{Media, PlayoutConfig, ProcessUnit::*},
    vec_strings,
};

/// Streaming Output
///
/// Prepare the ffmpeg command for streaming output
pub fn output(config: &PlayoutConfig, log_format: &str) -> process::Child {
    let mut enc_cmd = vec![];
    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();
    let mut media = Media::new(0, "", false);
    media.unit = Encoder;
    media.add_filter(config, &None);

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        log_format,
        "-re",
        "-i",
        "pipe:0"
    ];

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, &media.filter, config);

    debug!(
        "Encoder CMD: <bright-blue>\"ffmpeg {}\"</>",
        enc_cmd.join(" ")
    );

    let enc_proc = match Command::new("ffmpeg")
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
