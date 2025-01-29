/*
This module write the files compression directly to a hls (m3u8) playlist,
without pre- and post-processing.

Example config:

out:
    output_param: >-
        ...

        -flags +cgop
        -f hls
        -hls_time 6
        -hls_list_size 600
        -hls_flags append_list+delete_segments+omit_endlist+program_date_time
        -hls_segment_filename /var/www/html/live/stream-%d.ts /var/www/html/live/stream.m3u8

*/

use std::{process::Stdio, sync::atomic::Ordering};

use log::*;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::utils::{logging::log_line, task_runner};
use crate::vec_strings;
use crate::{
    player::{
        controller::{ChannelManager, ProcessUnit::*},
        input::source_generator,
        utils::{
            get_delta, is_free_tcp_port, prepare_output_cmd, sec_to_time, stderr_reader,
            valid_stream, Media,
        },
    },
    utils::{
        errors::ServiceError,
        logging::{fmt_cmd, Target},
    },
};

/// Ingest Server for HLS
async fn ingest_writer(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = manager.config.lock().await.clone();
    let id = config.general.channel_id;
    let playlist_init = manager.list_init.clone();
    let chain = manager.filter_chain.clone();
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut server_prefix = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let mut dummy_media = Media::new(0, "Live Stream", false).await;

    dummy_media.unit = Ingest;
    dummy_media.add_filter(&config, &chain).await;

    let is_alive = manager.is_alive.clone();
    let ingest_is_alive = manager.ingest_is_alive.clone();

    if let Some(ingest_input_cmd) = &config.advanced.ingest.input_cmd {
        server_prefix.append(&mut ingest_input_cmd.clone());
    }

    server_prefix.append(&mut stream_input.clone());

    if config.processing.vtt_enable {
        let vtt_dummy = config
            .channel
            .storage
            .join(config.processing.vtt_dummy.clone().unwrap_or_default());

        if vtt_dummy.is_file() {
            server_prefix.append(&mut vec_strings!["-i", vtt_dummy.to_string_lossy()]);
        }
    }

    let server_cmd = prepare_output_cmd(&config, server_prefix.clone(), &dummy_media.filter);
    let mut is_running;

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

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        info!(target: Target::file_mail(), channel = id; "Start ingest server, listening on: <b><magenta>{url}</></b>");
    };

    debug!(target: Target::file_mail(), channel = id;
        "Server CMD: <bright-blue>ffmpeg {}</>",
        fmt_cmd(&server_cmd)
    );

    loop {
        let mut level = &config.logging.ffmpeg_level;
        let mut server_proc = Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let server_err = BufReader::new(server_proc.stderr.take().unwrap());
        let mut lines = server_err.lines();

        *manager.ingest.lock().await = Some(server_proc);
        is_running = false;

        while let Some(line) = lines.next_line().await? {
            if line.contains("rtmp")
                && (line.contains("Unexpected stream") || line.contains("App field don't match up"))
                && !valid_stream(&line)
            {
                warn!(target: Target::file_mail(), channel = id; "Unexpected ingest stream: {line}");

                manager.stop(Ingest).await;
            } else if !is_running && line.contains("Input #0") {
                level = &config.logging.ingest_level;
                ingest_is_alive.store(true, Ordering::SeqCst);
                playlist_init.store(true, Ordering::SeqCst);
                is_running = true;

                info!(target: Target::file_mail(), channel = id; "Switch from {} to live ingest", config.processing.mode);

                manager.stop(Decoder).await;
            }

            log_line(id, &line, level);
        }

        if ingest_is_alive.load(Ordering::SeqCst) {
            info!(target: Target::file_mail(), channel = id; "Switch from live ingest to {}", config.processing.mode);
        }

        ingest_is_alive.store(false, Ordering::SeqCst);

        manager.wait(Ingest).await;

        if !is_alive.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}

async fn write(manager: &ChannelManager, ff_log_format: &str) -> Result<(), ServiceError> {
    let config = manager.config.lock().await.clone();
    let get_source = source_generator(manager.clone());
    let ingest_is_alive = manager.ingest_is_alive.clone();
    let id = config.general.channel_id;
    let current_media = manager.current_media.clone();
    let is_alive = manager.is_alive.clone();

    let mut get_source = get_source.await;

    while let Some(node) = get_source.next().await {
        *current_media.lock().await = Some(node.clone());
        let ignore = config.logging.ignore_lines.clone();

        if !is_alive.load(Ordering::SeqCst) {
            break;
        }

        let mut cmd = match &node.cmd {
            Some(cmd) => cmd.clone(),
            None => break,
        };

        if node.skip {
            continue;
        }

        info!(target: Target::file_mail(), channel = id;
            "Play for <yellow>{}</>: <b><magenta>{}</></b>",
            sec_to_time(node.out - node.seek),
            node.source
        );

        if config.task.enable {
            if config.task.path.is_file() {
                let manager3 = manager.clone();

                tokio::spawn(task_runner::run(manager3));
            } else {
                error!(target: Target::file_mail(), channel = id;
                    "<bright-blue>{:?}</> executable not exists!",
                    config.task.path
                );
            }
        }

        let mut dec_prefix = vec_strings!["-hide_banner", "-nostats", "-v", &ff_log_format];

        if let Some(decoder_input_cmd) = &config.advanced.decoder.input_cmd {
            dec_prefix.append(&mut decoder_input_cmd.clone());
        }

        let mut read_rate = 1.0;

        if let Some(begin) = &node.begin {
            let (delta, _) = get_delta(&config, begin);
            let duration = node.out - node.seek;
            let speed = duration / (duration + delta);

            if node.seek == 0.0
                && speed > 0.0
                && speed < 1.3
                && delta < config.general.stop_threshold
            {
                read_rate = speed;
            }
        }

        dec_prefix.append(&mut vec_strings!["-readrate", read_rate]);

        dec_prefix.append(&mut cmd);
        let dec_cmd = prepare_output_cmd(&config, dec_prefix, &node.filter);

        debug!(target: Target::file_mail(), channel = id;
            "HLS writer CMD: <bright-blue>ffmpeg {}</>",
            fmt_cmd(&dec_cmd)
        );

        let mut dec_proc = Command::new("ffmpeg")
            .args(dec_cmd)
            .kill_on_drop(true)
            .stderr(Stdio::piped())
            .spawn()?;

        let dec_err = BufReader::new(dec_proc.stderr.take().unwrap());
        *manager.decoder.lock().await = Some(dec_proc);

        stderr_reader(dec_err, ignore, Decoder, id).await?;

        manager.wait(Decoder).await;

        while ingest_is_alive.load(Ordering::SeqCst) {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    Ok(())
}

/// HLS Writer
///
/// Write with single ffmpeg instance directly to a HLS playlist.
pub async fn writer(manager: &ChannelManager, ff_log_format: &str) -> Result<(), ServiceError> {
    let config = manager.config.lock().await.clone();

    let manager2 = manager.clone();

    let handle_ingest = if config.ingest.enable {
        // spawn a thread for ffmpeg ingest server
        Some(tokio::spawn(ingest_writer(manager2)))
    } else {
        None
    };

    tokio::select! {
        result = async {
            if let Some(f) = handle_ingest {
                f.await?
            } else {
                Ok(())
            }
        }, if handle_ingest.is_some() => {
            result?;
        }

        result = write(manager, ff_log_format) => {
            result?;
        }
    }

    Ok(())
}
