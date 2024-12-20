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
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    thread::{self, sleep},
    time::{Duration, SystemTime},
};

use log::*;

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
    utils::{errors::ProcessError, logging::Target},
};

/// Ingest Server for HLS
fn ingest_to_hls_server(manager: ChannelManager) -> Result<(), ProcessError> {
    let config = manager.config.lock().unwrap();
    let id = config.general.channel_id;
    let playlist_init = manager.list_init.clone();
    let chain = manager.filter_chain.clone();
    let mut error_count = 0;

    let mut server_prefix = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut dummy_media = Media::new(0, "Live Stream", false);
    dummy_media.unit = Ingest;

    let is_terminated = manager.is_terminated.clone();
    let ingest_is_running = manager.ingest_is_running.clone();

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

    let mut is_running;

    if let Some(url) = stream_input.iter().find(|s| s.contains("://")) {
        if is_free_tcp_port(id, url) {
            info!(target: Target::file_mail(), channel = id; "Start ingest server, listening on: <b><magenta>{url}</></b>");
        } else {
            manager.channel.lock().unwrap().active = false;
            manager.stop_all();
        }
    };

    drop(config);

    loop {
        let config = manager.config.lock().unwrap().clone();
        dummy_media.add_filter(&config, &chain);
        let server_cmd = prepare_output_cmd(&config, server_prefix.clone(), &dummy_media.filter);
        let timer = SystemTime::now();

        debug!(target: Target::file_mail(), channel = id;
            "Server CMD: <bright-blue>\"ffmpeg {}\"</>",
            server_cmd.join(" ")
        );

        let proc_ctl = manager.clone();
        let mut server_proc = match Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!(target: Target::file_mail(), channel = id; "couldn't spawn ingest server: {e}");
                panic!("couldn't spawn ingest server: {e}");
            }
            Ok(proc) => proc,
        };

        let server_err = BufReader::new(server_proc.stderr.take().unwrap());
        *manager.ingest.lock().unwrap() = Some(server_proc);
        is_running = false;

        for line in server_err.lines() {
            let line = line?;

            if line.contains("rtmp") && line.contains("Unexpected stream") && !valid_stream(&line) {
                warn!(target: Target::file_mail(), channel = id; "Unexpected ingest stream: {line}");

                if let Err(e) = proc_ctl.stop(Ingest) {
                    error!(target: Target::file_mail(), channel = id; "{e}");
                };
            }

            if !is_running && line.contains("Input #0") {
                ingest_is_running.store(true, Ordering::SeqCst);
                playlist_init.store(true, Ordering::SeqCst);
                is_running = true;

                info!(target: Target::file_mail(), channel = id; "Switch from {} to live ingest", config.processing.mode);

                if let Err(e) = manager.stop(Decoder) {
                    error!(target: Target::file_mail(), channel = id; "{e}");
                }
            }

            if ingest_is_running.load(Ordering::SeqCst) {
                log_line(&line, &config.logging.ingest_level);
            } else {
                log_line(&line, &config.logging.ffmpeg_level);
            }
        }

        if ingest_is_running.load(Ordering::SeqCst) {
            info!(target: Target::file_mail(), channel = id; "Switch from live ingest to {}", config.processing.mode);
        }

        ingest_is_running.store(false, Ordering::SeqCst);

        if let Err(e) = manager.wait(Ingest) {
            error!(target: Target::file_mail(), channel = id; "{e}");
        }

        if is_terminated.load(Ordering::SeqCst) {
            break;
        }

        if let Ok(elapsed) = timer.elapsed() {
            if elapsed.as_millis() < 300 {
                error_count += 1;

                if error_count > 10 {
                    error!(target: Target::file_mail(), channel = id; "Reach fatal error count in ingest, terminate channel!");
                    manager.channel.lock().unwrap().active = false;
                    manager.stop_all();
                    break;
                }
            } else {
                error_count = 0;
            }
        }
    }

    Ok(())
}

/// HLS Writer
///
/// Write with single ffmpeg instance directly to a HLS playlist.
pub fn write_hls(manager: ChannelManager) -> Result<(), ProcessError> {
    let config = manager.config.lock()?.clone();
    let id = config.general.channel_id;
    let current_media = manager.current_media.clone();
    let is_terminated = manager.is_terminated.clone();

    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());

    let channel_mgr_2 = manager.clone();
    let ingest_is_running = manager.ingest_is_running.clone();

    let get_source = source_generator(manager.clone());

    // spawn a thread for ffmpeg ingest server and create a channel for package sending
    if config.ingest.enable {
        thread::spawn(move || ingest_to_hls_server(channel_mgr_2));
    }

    let mut error_count = 0;

    for node in get_source {
        *current_media.lock().unwrap() = Some(node.clone());
        let ignore = config.logging.ignore_lines.clone();
        let timer = SystemTime::now();

        if is_terminated.load(Ordering::SeqCst) {
            break;
        }

        let mut cmd = match &node.cmd {
            Some(cmd) => cmd.clone(),
            None => break,
        };

        if !node.process.unwrap() {
            continue;
        }

        info!(target: Target::file_mail(), channel = id;
            "Play for <yellow>{}</>: <b><magenta>{}</></b>",
            sec_to_time(node.out - node.seek),
            node.source
        );

        if config.task.enable {
            if config.task.path.is_file() {
                let channel_mgr_3 = manager.clone();

                thread::spawn(move || task_runner::run(channel_mgr_3));
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
            "HLS writer CMD: <bright-blue>\"ffmpeg {}\"</>",
            dec_cmd.join(" ")
        );

        let mut dec_proc = match Command::new("ffmpeg")
            .args(dec_cmd)
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(proc) => proc,
            Err(e) => {
                error!(target: Target::file_mail(), channel = id; "couldn't spawn ffmpeg process: {e}");
                panic!("couldn't spawn ffmpeg process: {e}")
            }
        };

        let dec_err = BufReader::new(dec_proc.stderr.take().unwrap());
        *manager.decoder.lock().unwrap() = Some(dec_proc);

        if let Err(e) = stderr_reader(dec_err, ignore, Decoder, manager.clone()) {
            error!(target: Target::file_mail(), channel = id; "{e:?}");
        };

        if let Err(e) = manager.wait(Decoder) {
            error!(target: Target::file_mail(), channel = id; "{e}");
        }

        while ingest_is_running.load(Ordering::SeqCst) {
            sleep(Duration::from_secs(1));
        }

        if let Ok(elapsed) = timer.elapsed() {
            if elapsed.as_millis() < 300 {
                error_count += 1;

                if error_count > 10 {
                    error!(target: Target::file_mail(), channel = id; "Reach fatal error count, terminate channel!");
                    break;
                }
            } else {
                error_count = 0;
            }
        }
    }

    sleep(Duration::from_secs(1));

    manager.stop_all();

    Ok(())
}
