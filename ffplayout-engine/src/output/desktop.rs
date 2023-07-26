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
        "-i",
        "pipe:0",
        "-window_title",
        "ffplayout"
    ];

    if let Some(mut cmd) = config.out.output_cmd.clone() {
        if !cmd.iter().any(|i| {
            [
                "-c",
                "-c:v",
                "-c:v:0",
                "-b:v",
                "-b:v:0",
                "-vcodec",
                "-c:a",
                "-acodec",
                "-crf",
                "-map",
                "-filter_complex",
            ]
            .contains(&i.as_str())
        }) {
            enc_cmd.append(&mut cmd);
        } else {
            warn!("Given output parameters are skipped, they are not supported by ffplay!");
        }
    }

    if config.text.add_text && !config.text.text_from_filename && !config.processing.audio_only {
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

    enc_cmd.append(&mut enc_filter);

    debug!(
        "Encoder CMD: <bright-blue>\"ffplay {}\"</>",
        enc_cmd.join(" ")
    );

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
