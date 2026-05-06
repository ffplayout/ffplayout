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

use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

use log::*;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::{Duration, sleep},
};
use tokio_util::sync::CancellationToken;

use crate::{
    player::{
        controller::{ChannelManager, ProcessUnit::*},
        input::source_generator,
        utils::{
            Media, get_delta, insert_readrate, is_free_tcp_port, prepare_output_cmd, sec_to_time,
            stderr_reader, valid_stream,
        },
    },
    utils::{
        errors::ServiceError,
        logging::{Target, fmt_cmd, log_line},
        task_runner,
    },
    vec_strings,
};

fn log_dev_task(enabled: bool, channel_id: i32, task: &str, event: &str) {
    if enabled {
        debug!(target: Target::file(), channel = channel_id; "<span class=\"log-gray\">[Dev Metrics]</span> task=<span class=\"log-addr\">{task}</span> event=<span class=\"log-addr\">{event}</span>");
    }
}

/// Periodically checks if HLS segments are still being updated.
/// If no new segment is written for longer than `hls_time * 3`, returns an error.
pub async fn hls_watchdog(
    channel_id: i32,
    m3u8_path: PathBuf,
    hls_time: Duration,
    is_alive: Arc<AtomicBool>,
    cancel_token: CancellationToken,
) -> Result<(), ServiceError> {
    let mut init = true;
    let timeout = hls_time * 3;

    loop {
        if cancel_token.is_cancelled() {
            break;
        }
        let sleep_time = if init {
            init = false;
            timeout
        } else {
            hls_time
        };

        tokio::select! {
            _ = cancel_token.cancelled() => break,
            _ = sleep(sleep_time) => {}
        }

        if cancel_token.is_cancelled() || !is_alive.load(Ordering::SeqCst) {
            break;
        }

        if let Ok(last_mod) = m3u8_path.metadata().and_then(|m| m.modified()) {
            let age = SystemTime::now()
                .duration_since(last_mod)
                .unwrap_or_default();

            if age > timeout {
                error!(target: Target::file_mail(), channel = channel_id;
                    "HLS segment write timeout! Last update: <span class=\"log-number\">{:.3}s</span>", age.as_secs_f32()
                );
                return Err(ServiceError::Conflict("Timeout".to_string()));
            }
        }
    }

    Ok(())
}

/// Ingest Server for HLS
async fn ingest_writer(
    manager: ChannelManager,
    cancel_token: CancellationToken,
) -> Result<(), ServiceError> {
    let config = manager.config.read().await.clone();
    let id = config.general.channel_id;
    let playlist_init = manager.list_init.clone();
    let chain = manager.filter_chain.clone();
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut server_prefix = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+info",
        "-timeout",
        "0"
    ];
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

            error!(target: Target::file_mail(), channel = id; "Address <span class=\"log-addr\">{url}</span> already in use!");

            manager.stop(Ingest).await;

            if num >= 4 {
                return Err(ServiceError::Conflict(
                    "Can't run ingest server!".to_string(),
                ));
            }

            tokio::select! {
                _ = cancel_token.cancelled() => return Ok(()),
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {}
            }
        }

        info!(target: Target::file_mail(), channel = id; "Start ingest server, listening on: <span class=\"log-addr\">{url}</span>");
    };

    debug!(target: Target::file_mail(), channel = id;
        "Server CMD: <span class=\"log-cmd\">ffmpeg {}</span>",
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

        loop {
            let line = tokio::select! {
                _ = cancel_token.cancelled() => break,
                line = lines.next_line() => line?,
            };

            let Some(line) = line else {
                break;
            };
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

        if cancel_token.is_cancelled() || !is_alive.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}

async fn write(
    manager: &ChannelManager,
    ff_log_format: &str,
    cancel_token: CancellationToken,
) -> Result<(), ServiceError> {
    let config = manager.config.read().await.clone();
    let get_source = source_generator(manager.clone());
    let ingest_is_alive = manager.ingest_is_alive.clone();
    let id = config.general.channel_id;
    let current_media = manager.current_media.clone();
    let is_alive = manager.is_alive.clone();

    let mut get_source = get_source.await;

    while let Some(node) = get_source.next().await {
        *current_media.lock().await = Some(node.clone());
        let ignore = config.logging.ignore_lines.clone();

        if cancel_token.is_cancelled() || !is_alive.load(Ordering::SeqCst) {
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
            "Play for <span class=\"log-number\">{}</span>: <span class=\"log-addr\">{}</span>",
            sec_to_time(node.out - node.seek),
            node.source
        );

        if config.task.enable {
            if config.task.path.is_file() {
                let manager3 = manager.clone();

                tokio::spawn(task_runner::run(manager3));
            } else {
                error!(target: Target::file_mail(), channel = id;
                    "<span class=\"log-cmd\">{:?}</span> executable not exists!",
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

        insert_readrate(&config.general.ffmpeg_options, &mut cmd, read_rate);

        dec_prefix.append(&mut cmd);
        let dec_cmd = prepare_output_cmd(&config, dec_prefix, &node.filter);

        debug!(target: Target::file_mail(), channel = id;
            "HLS writer CMD: <span class=\"log-cmd\">ffmpeg {}</span>",
            fmt_cmd(&dec_cmd)
        );

        let mut dec_proc = Command::new("ffmpeg")
            .args(dec_cmd)
            .kill_on_drop(true)
            .stderr(Stdio::piped())
            .spawn()?;

        let dec_err = BufReader::new(dec_proc.stderr.take().unwrap());
        *manager.decoder.lock().await = Some(dec_proc);

        stderr_reader(dec_err, ignore, Decoder, id, cancel_token.clone()).await?;

        manager.wait(Decoder).await;

        while ingest_is_alive.load(Ordering::SeqCst) {
            tokio::select! {
                _ = cancel_token.cancelled() => return Ok(()),
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {}
            }
        }
    }

    Ok(())
}

/// HLS Writer
///
/// Write with single ffmpeg instance directly to a HLS playlist.
pub async fn writer(manager: &ChannelManager, ff_log_format: &str) -> Result<(), ServiceError> {
    let config = manager.config.read().await.clone();
    let manager2 = manager.clone();
    let is_alive = manager.is_alive.clone();
    let output_cmd = config.output.output_cmd.unwrap_or_default();
    let dev_metrics = config.general.dev_metrics;
    let hls_duration = output_cmd
        .windows(2)
        .find_map(|pair| {
            if pair[0] == "-hls_time" {
                pair[1].parse::<u64>().ok()
            } else {
                None
            }
        })
        .unwrap_or(10);

    let m3u8_path = output_cmd
        .iter()
        .find(|s| s.ends_with(".m3u8") && !s.contains("master.m3u8"))
        .map(PathBuf::from)
        .unwrap_or(config.channel.public.clone());

    let ingest_token = CancellationToken::new();
    let watchdog_token = CancellationToken::new();

    let mut handle_ingest = if config.ingest.enable {
        // spawn a thread for ffmpeg ingest server
        log_dev_task(
            dev_metrics,
            config.general.channel_id,
            "hls_ingest_writer",
            "start",
        );
        Some(tokio::spawn(Box::pin(ingest_writer(
            manager2,
            ingest_token.clone(),
        ))))
    } else {
        None
    };

    log_dev_task(
        dev_metrics,
        config.general.channel_id,
        "hls_watchdog",
        "start",
    );
    let mut watchdog_hls = tokio::spawn(hls_watchdog(
        config.general.channel_id,
        m3u8_path,
        Duration::from_secs(hls_duration),
        is_alive,
        watchdog_token.clone(),
    ));

    let result: Result<(), ServiceError> = tokio::select! {
        result = async {
            if let Some(f) = &mut handle_ingest {
                f.await?
            } else {
                Ok(())
            }
        }, if handle_ingest.is_some() => {
            result?;
            Ok(())
        }

        result = &mut watchdog_hls => {
            result??;
            Ok(())
        }

        result = write(manager, ff_log_format, watchdog_token.clone()) => {
            result?;
            Ok(())
        }
    };

    watchdog_token.cancel();
    ingest_token.cancel();
    log_dev_task(
        dev_metrics,
        config.general.channel_id,
        "hls_watchdog",
        "cancel_requested",
    );
    if handle_ingest.is_some() {
        log_dev_task(
            dev_metrics,
            config.general.channel_id,
            "hls_ingest_writer",
            "cancel_requested",
        );
    }

    tokio::select! {
        _ = &mut watchdog_hls => {
            log_dev_task(dev_metrics, config.general.channel_id, "hls_watchdog", "done");
        }
        _ = sleep(Duration::from_secs(2)) => {
            log_dev_task(dev_metrics, config.general.channel_id, "hls_watchdog", "abort_fallback");
            watchdog_hls.abort();
            let _ = watchdog_hls.await;
        }
    }

    if let Some(mut handle_ingest) = handle_ingest {
        tokio::select! {
            _ = &mut handle_ingest => {
                log_dev_task(dev_metrics, config.general.channel_id, "hls_ingest_writer", "done");
            }
            _ = sleep(Duration::from_secs(2)) => {
                log_dev_task(dev_metrics, config.general.channel_id, "hls_ingest_writer", "abort_fallback");
                handle_ingest.abort();
                let _ = handle_ingest.await;
            }
        }
    }

    result?;

    Ok(())
}
