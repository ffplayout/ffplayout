use std::process::{self, Command, Stdio};

use log::*;

use crate::player::{
    controller::ProcessUnit::*,
    utils::{prepare_output_cmd, Media},
};
use crate::utils::{config::PlayoutConfig, logging::Target};
use crate::vec_strings;

/// Desktop Output
///
/// Instead of streaming, we run a ffplay instance and play on desktop.
pub fn output(config: &PlayoutConfig, log_format: &str) -> process::Child {
    let mut media = Media::new(0, "", false);
    let id = config.general.channel_id;
    media.unit = Encoder;
    media.add_filter(config, &None);

    let mut enc_prefix = vec_strings!["-hide_banner", "-nostats", "-v", log_format];

    if let Some(input_cmd) = &config.advanced.encoder.input_cmd {
        enc_prefix.append(&mut input_cmd.clone());
    }

    enc_prefix.append(&mut vec_strings!["-re", "-i", "pipe:0"]);

    let enc_cmd = prepare_output_cmd(config, enc_prefix, &media.filter);

    debug!(target: Target::file_mail(), channel = id;
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
            error!(target: Target::file_mail(), channel = id; "couldn't spawn encoder process: {e}");
            panic!("couldn't spawn encoder process: {e}")
        }
        Ok(proc) => proc,
    };

    enc_proc
}
