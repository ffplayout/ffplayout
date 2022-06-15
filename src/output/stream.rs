use std::process::{self, Command, Stdio};

use simplelog::*;

use crate::filter::v_drawtext;
use crate::utils::{prepare_output_cmd, Media, PlayoutConfig};
use crate::vec_strings;

/// Streaming Output
///
/// Prepare the ffmpeg command for streaming output
pub fn output(config: &PlayoutConfig, log_format: &str) -> process::Child {
    let mut enc_cmd = vec![];
    let mut enc_filter = vec![];
    let mut preview_cmd = config.out.preview_cmd.as_ref().unwrap().clone();
    let mut output_cmd = config.out.output_cmd.as_ref().unwrap().clone();

    let enc_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        log_format,
        "-re",
        "-i",
        "pipe:0"
    ];

    if config.text.add_text && !config.text.text_from_filename {
        if let Some(socket) = config.text.bind_address.clone() {
            debug!(
                "Using drawtext filter, listening on address: <yellow>{}</>",
                socket
            );

            let mut filter = "[0:v]null,".to_string();

            filter.push_str(
                v_drawtext::filter_node(config, &Media::new(0, String::new(), false)).as_str(),
            );

            enc_filter = vec!["-filter_complex".to_string(), filter];
        }
    }

    if config.out.preview {
        enc_cmd.append(&mut preview_cmd);
    }

    enc_cmd.append(&mut output_cmd);

    let enc_cmd = prepare_output_cmd(enc_prefix, enc_filter, enc_cmd, &config.out.mode);

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
