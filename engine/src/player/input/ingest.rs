use std::{process::Stdio, sync::atomic::Ordering};

use log::*;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{ChildStderr, Command},
};

use crate::utils::{
    config::{PlayoutConfig, FFMPEG_IGNORE_ERRORS, FFMPEG_UNRECOVERABLE_ERRORS},
    logging::{log_line, Target},
};
use crate::vec_strings;
use crate::{
    player::{
        controller::{ChannelManager, ProcessUnit::*},
        utils::{is_free_tcp_port, valid_stream, Media},
    },
    utils::{errors::ServiceError, logging::fmt_cmd},
};

async fn server_monitor(
    id: i32,
    level: String,
    ignore: Vec<String>,
    buffer: BufReader<ChildStderr>,
    manager: ChannelManager,
) -> Result<(), ServiceError> {
    let mut is_running = false;

    let mut lines = buffer.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if !FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i))
            && !ignore.iter().any(|i| line.contains(i))
        {
            log_line(id, &line, &level);
        }

        if line.contains("rtmp")
            && (line.contains("Unexpected stream") || line.contains("App field don't match up"))
            && !valid_stream(&line)
        {
            warn!(target: Target::file_mail(), channel = id; "Unexpected ingest stream: {line}");

            manager.stop(Ingest).await;

            break;
        } else if !is_running {
            manager.ingest_is_alive.store(true, Ordering::SeqCst);

            is_running = true;
        }

        if FFMPEG_UNRECOVERABLE_ERRORS
            .iter()
            .any(|i| line.contains(*i))
        {
            error!(target: Target::file_mail(), channel = id; "Hit unrecoverable error!");
            manager.channel.lock().await.active = false;
            manager.stop_all(false).await;
        }
    }

    Ok(())
}

/// ffmpeg Ingest Server
///
/// Start ffmpeg in listen mode, and wait for input.
pub async fn ingest_server(
    config: PlayoutConfig,
    manager: ChannelManager,
) -> Result<(), ServiceError> {
    let id = config.general.channel_id;
    let mut server_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut dummy_media = Media::new(0, "Live Stream", false).await;
    dummy_media.unit = Ingest;
    dummy_media.add_filter(&config, &None).await;
    let is_alive = manager.is_alive.clone();
    let ingest_is_alive = manager.ingest_is_alive.clone();
    let vtt_dummy = config
        .channel
        .storage
        .join(config.processing.vtt_dummy.clone().unwrap_or_default());

    if let Some(ingest_input_cmd) = config.advanced.ingest.input_cmd {
        server_cmd.append(&mut ingest_input_cmd.clone());
    }

    server_cmd.append(&mut stream_input.clone());

    if config.processing.vtt_enable && vtt_dummy.is_file() {
        server_cmd.append(&mut vec_strings!["-i", vtt_dummy.to_string_lossy()]);
    }

    if let Some(mut filter) = dummy_media.filter {
        server_cmd.append(&mut filter.cmd());
        server_cmd.append(&mut filter.map());
    }

    if config.processing.vtt_enable && vtt_dummy.is_file() {
        server_cmd.append(&mut vec_strings!("-map", "1:s"));
    }

    if let Some(mut cmd) = config.processing.cmd {
        server_cmd.append(&mut cmd);
    }

    debug!(target: Target::file_mail(), channel = id;
        "Server CMD: <bright-blue>ffmpeg {}</>",
        fmt_cmd(&server_cmd)
    );

    if let Some(url) = stream_input.iter().find(|s| s.contains("://")) {
        for num in 0..5 {
            if is_free_tcp_port(url) {
                break;
            }

            error!(target: Target::file_mail(), channel = id; "Address <b><magenta>{url}</></b> already in use!");

            if num >= 4 {
                manager.channel.lock().await.active = false;

                return Err(ServiceError::Conflict(
                    "Can't run ingest server!".to_string(),
                ));
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }

        info!(target: Target::file_mail(), channel = id; "Start ingest server, listening on: <b><magenta>{url}</></b>");
    };

    while is_alive.load(Ordering::SeqCst) {
        let proc_ctl = manager.clone();
        let level = config.logging.ingest_level.clone();
        let ignore = config.logging.ignore_lines.clone();
        let mut server_proc = Command::new("ffmpeg")
            .args(server_cmd.clone())
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let ingest_stdout = server_proc.stdout.take().unwrap();
        let server_err = BufReader::new(server_proc.stderr.take().unwrap());

        *manager.ingest_stdout.lock().await = Some(ingest_stdout);
        *manager.ingest.lock().await = Some(server_proc);

        server_monitor(id, level, ignore, server_err, proc_ctl).await?;
        ingest_is_alive.store(false, Ordering::SeqCst);

        manager.wait(Ingest).await;

        trace!("Restart ingest server");
    }

    Ok(())
}
