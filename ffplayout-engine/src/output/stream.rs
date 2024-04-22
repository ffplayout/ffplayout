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
    let mut media = Media::new(0, "", false);
    media.unit = Encoder;
    media.add_filter(config, &None);

    let mut enc_prefix = vec_strings!["-hide_banner", "-nostats", "-v", log_format];

    if let Some(input_cmd) = config
        .advanced
        .as_ref()
        .and_then(|a| a.encoder.input_cmd.clone())
    {
        enc_prefix.append(&mut input_cmd.clone());
    }

    enc_prefix.append(&mut vec_strings!["-re", "-i", "pipe:0"]);

    let enc_cmd = prepare_output_cmd(config, enc_prefix, &media.filter);

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
