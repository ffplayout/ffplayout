use std::{process::Stdio, sync::atomic::Ordering};

use log::*;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{ChildStderr, Command},
    sync::mpsc::Sender,
};

use crate::utils::{
    config::{PlayoutConfig, FFMPEG_IGNORE_ERRORS, FFMPEG_UNRECOVERABLE_ERRORS},
    logging::{log_line, Target},
};
use crate::vec_strings;
use crate::{
    player::{
        controller::{ChannelManager, ProcessUnit::*},
        utils::{test_tcp_port, valid_stream, Media},
    },
    utils::errors::ProcessError,
};

async fn server_monitor(
    id: i32,
    level: &str,
    ignore: Vec<String>,
    buffer: BufReader<ChildStderr>,
    channel_mgr: ChannelManager,
) -> Result<(), ProcessError> {
    let mut lines = buffer.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if !FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i))
            && !ignore.iter().any(|i| line.contains(i))
        {
            log_line(&line, level);
        }

        if line.contains("rtmp") && line.contains("Unexpected stream") && !valid_stream(&line) {
            if let Err(e) = channel_mgr.stop(Ingest).await {
                error!(target: Target::file_mail(), channel = id; "{e}");
            };
        }

        if FFMPEG_UNRECOVERABLE_ERRORS
            .iter()
            .any(|i| line.contains(*i))
        {
            error!(target: Target::file_mail(), channel = id; "Hit unrecoverable error!");
            channel_mgr.channel.lock().await.active = false;
            channel_mgr.stop_all().await;
        }
    }

    Ok(())
}

/// ffmpeg Ingest Server
///
/// Start ffmpeg in listen mode, and wait for input.
pub async fn ingest_server(
    config: PlayoutConfig,
    ingest_sender: Sender<(usize, [u8; 65088])>,
    channel_mgr: ChannelManager,
) -> Result<(), ProcessError> {
    let id = config.general.channel_id;
    let mut buffer: [u8; 65088] = [0; 65088];
    let mut server_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut dummy_media = Media::new(0, "Live Stream", false);
    dummy_media.unit = Ingest;
    dummy_media.add_filter(&config, &None).await;
    let is_terminated = channel_mgr.is_terminated.clone();
    let ingest_is_running = channel_mgr.ingest_is_running.clone();
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

    let mut is_running;

    if let Some(url) = stream_input.iter().find(|s| s.contains("://")) {
        if !test_tcp_port(id, url) {
            channel_mgr.channel.lock().await.active = false;
            channel_mgr.stop_all().await;
        }

        info!(target: Target::file_mail(), channel = id; "Start ingest server, listening on: <b><magenta>{url}</></b>",);
    };

    debug!(target: Target::file_mail(), channel = id;
        "Server CMD: <bright-blue>\"ffmpeg {}\"</>",
        server_cmd.join(" ")
    );

    while !is_terminated.load(Ordering::SeqCst) {
        let proc_ctl = channel_mgr.clone();
        let level = config.logging.ingest_level.clone();
        let ignore = config.logging.ignore_lines.clone();
        let mut server_proc = match Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!(target: Target::file_mail(), channel = id; "couldn't spawn ingest server: {e}");
                panic!("couldn't spawn ingest server: {e}")
            }
            Ok(proc) => proc,
        };
        let mut ingest_reader = BufReader::new(server_proc.stdout.take().unwrap());
        let server_err = BufReader::new(server_proc.stderr.take().unwrap());
        let error_reader_thread =
            tokio::spawn(
                async move { server_monitor(id, &level, ignore, server_err, proc_ctl).await },
            );

        *channel_mgr.ingest.lock().await = Some(server_proc);
        is_running = false;

        loop {
            let bytes_len = match ingest_reader.read(&mut buffer[..]).await {
                Ok(length) => length,
                Err(e) => {
                    debug!(target: Target::file_mail(), channel = id; "Ingest server read {e:?}");
                    break;
                }
            };

            if !is_running {
                ingest_is_running.store(true, Ordering::SeqCst);
                is_running = true;
            }

            if bytes_len > 0 {
                if let Err(e) = ingest_sender.send((bytes_len, buffer)).await {
                    error!(target: Target::file_mail(), channel = id; "Ingest server write error: {e:?}");

                    is_terminated.store(true, Ordering::SeqCst);
                    break;
                }
            } else {
                break;
            }
        }

        drop(ingest_reader);
        ingest_is_running.store(false, Ordering::SeqCst);

        if let Err(e) = channel_mgr.wait(Ingest).await {
            error!(target: Target::file_mail(), channel = id; "{e}")
        }

        if let Err(e) = error_reader_thread.await {
            error!(target: Target::file_mail(), channel = id; "{e:?}");
        };
    }

    Ok(())
}