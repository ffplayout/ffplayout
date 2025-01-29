use std::process::Stdio;

use log::*;
use tokio::process::{Child, Command};

use crate::player::filter::v_drawtext;
use crate::utils::errors::ServiceError;
use crate::utils::{
    config::PlayoutConfig,
    logging::{fmt_cmd, Target},
};
use crate::vec_strings;

/// Desktop Output
///
/// Instead of streaming, we run a ffplay instance and play on desktop.
pub async fn output(config: &PlayoutConfig, log_format: &str) -> Result<Child, ServiceError> {
    let mut enc_filter: Vec<String> = vec![];
    let mut enc_cmd = vec_strings!["-hide_banner", "-nostats", "-v", log_format];

    if let Some(encoder_input_cmd) = &config.advanced.encoder.input_cmd {
        enc_cmd.append(&mut encoder_input_cmd.clone());
    }

    enc_cmd.append(&mut vec_strings![
        "-autoexit",
        "-i",
        "pipe:0",
        "-window_title",
        "ffplayout"
    ]);

    if let Some(mut cmd) = config.output.output_cmd.clone() {
        if cmd.iter().any(|i| {
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
            warn!(target: Target::file_mail(), channel = config.general.channel_id; "ffplay doesn't support given output parameters, they will be skipped!");
        } else {
            enc_cmd.append(&mut cmd);
        }
    }

    if config.text.add_text && !config.text.text_from_filename && !config.processing.audio_only {
        if let Some(socket) = config.text.zmq_stream_socket.clone() {
            debug!(target: Target::file_mail(), channel = config.general.channel_id;
                "Using drawtext filter, listening on address: <yellow>{}</>",
                socket
            );

            let mut filter: String = "null,".to_string();
            filter.push_str(v_drawtext::filter_node(config, None, &None).await.as_str());
            enc_filter = vec!["-vf".to_string(), filter];
        }
    }

    enc_cmd.append(&mut enc_filter);

    debug!(target: Target::file_mail(), channel = config.general.channel_id;
        "Encoder CMD: <bright-blue>ffplay {}</>",
        fmt_cmd(&enc_cmd)
    );

    let child = Command::new("ffplay")
        .args(enc_cmd)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    Ok(child)
}
