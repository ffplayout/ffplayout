use std::process::{self, Command, Stdio};

use simplelog::*;

use ffplayout_lib::{filter::v_drawtext, utils::PlayoutConfig, vec_strings};

/// Desktop Output
///
/// Instead of streaming, we run a ffplay instance and play on desktop.
pub fn output(config: &PlayoutConfig, log_format: &str) -> process::Child {
    let mut enc_filter: Vec<String> = vec![];

    let mut enc_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        log_format,
        "-re",
        "-i",
        "pipe:0",
        "-f",
        "null",
        "-"
    ];

    if config.text.add_text && !config.text.text_from_filename {
        if let Some(socket) = config.text.zmq_stream_socket.clone() {
            debug!(
                "Using drawtext filter, listening on address: <yellow>{}</>",
                socket
            );

            let mut filter: String = "null,".to_string();
            filter.push_str(v_drawtext::filter_node(config, None, &None).as_str());
            enc_filter = vec!["-vf".to_string(), filter];
        }
    }

    enc_cmd.splice(7..7, enc_filter);

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
