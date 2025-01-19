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
    channel_mgr: ChannelManager,
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

            if let Err(e) = channel_mgr.stop(Ingest).await {
                error!(target: Target::file_mail(), channel = id; "{e}");
            };

            break;
        } else if !is_running {
            channel_mgr.ingest_is_alive.store(true, Ordering::SeqCst);

            is_running = true;
        }

        if FFMPEG_UNRECOVERABLE_ERRORS
            .iter()
            .any(|i| line.contains(*i))
        {
            error!(target: Target::file_mail(), channel = id; "Hit unrecoverable error!");
            channel_mgr.channel.lock().await.active = false;
            channel_mgr.stop_all(false).await?;
        }
    }

    Ok(())
}

/// ffmpeg Ingest Server
///
/// Start ffmpeg in listen mode, and wait for input.
pub async fn ingest_server(
    config: PlayoutConfig,
    channel_mgr: ChannelManager,
) -> Result<(), ServiceError> {
    let id = config.general.channel_id;
    let mut server_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut dummy_media = Media::new(0, "Live Stream", false).await;
    dummy_media.unit = Ingest;
    dummy_media.add_filter(&config, &None).await;
    let is_alive = channel_mgr.is_alive.clone();
    let ingest_is_alive = channel_mgr.ingest_is_alive.clone();
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
        let mut attempts = 0;

        while attempts < 5 {
            if is_free_tcp_port(id, url) {
                info!(target: Target::file_mail(), channel = id; "Start ingest server, listening on: <b><magenta>{url}</></b>");
                break;
            }

            attempts += 1;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        if attempts == 5 {
            channel_mgr.channel.lock().await.active = false;
            channel_mgr.stop_all(false).await?;
        }
    };

    while is_alive.load(Ordering::SeqCst) {
        let proc_ctl = channel_mgr.clone();
        let level = config.logging.ingest_level.clone();
        let ignore = config.logging.ignore_lines.clone();
        let mut server_proc = match Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Err(e) => {
                error!(target: Target::file_mail(), channel = id; "couldn't spawn ingest server: {e}");
                panic!("couldn't spawn ingest server: {e}")
            }
            Ok(proc) => proc,
        };
        let ingest_stdout = server_proc.stdout.take().unwrap();
        let server_err = BufReader::new(server_proc.stderr.take().unwrap());

        *channel_mgr.ingest_stdout.lock().await = Some(ingest_stdout);
        *channel_mgr.ingest.lock().await = Some(server_proc);

        server_monitor(id, level, ignore, server_err, proc_ctl).await?;
        ingest_is_alive.store(false, Ordering::SeqCst);

        if let Err(e) = channel_mgr.wait(Ingest).await {
            error!(target: Target::file_mail(), channel = id; "{e}");
        }

        trace!("Restart ingest server");
    }

    Ok(())
}
