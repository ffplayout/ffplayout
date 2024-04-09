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
    io::{BufRead, BufReader, Error},
    process::{exit, Command, Stdio},
    sync::atomic::Ordering,
    thread::{self, sleep},
    time::Duration,
};

use simplelog::*;

use crate::input::source_generator;
use crate::utils::{log_line, prepare_output_cmd, task_runner, valid_stream};
use ffplayout_lib::{
    utils::{
        controller::ProcessUnit::*, get_delta, sec_to_time, stderr_reader, test_tcp_port, Media,
        PlayerControl, PlayoutConfig, PlayoutStatus, ProcessControl,
    },
    vec_strings, ADVANCED_CONFIG,
};

/// Ingest Server for HLS
fn ingest_to_hls_server(
    config: PlayoutConfig,
    playout_stat: PlayoutStatus,
    proc_control: ProcessControl,
) -> Result<(), Error> {
    let playlist_init = playout_stat.list_init;

    let mut server_prefix = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut dummy_media = Media::new(0, "Live Stream", false);
    dummy_media.unit = Ingest;

    if let Some(ingest_input_cmd) = &ADVANCED_CONFIG.ingest.input_cmd {
        server_prefix.append(&mut ingest_input_cmd.clone());
    }

    server_prefix.append(&mut stream_input.clone());

    let mut is_running;

    if let Some(url) = stream_input.iter().find(|s| s.contains("://")) {
        if !test_tcp_port(url) {
            proc_control.stop_all();
            exit(1);
        }

        info!("Start ingest server, listening on: <b><magenta>{url}</></b>");
    };

    loop {
        dummy_media.add_filter(&config, &playout_stat.chain);
        let server_cmd = prepare_output_cmd(&config, server_prefix.clone(), &dummy_media.filter);

        debug!(
            "Server CMD: <bright-blue>\"ffmpeg {}\"</>",
            server_cmd.join(" ")
        );

        let proc_ctl = proc_control.clone();
        let mut server_proc = match Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!("couldn't spawn ingest server: {e}");
                panic!("couldn't spawn ingest server: {e}");
            }
            Ok(proc) => proc,
        };

        let server_err = BufReader::new(server_proc.stderr.take().unwrap());
        *proc_control.server_term.lock().unwrap() = Some(server_proc);
        is_running = false;

        for line in server_err.lines() {
            let line = line?;

            if line.contains("rtmp") && line.contains("Unexpected stream") && !valid_stream(&line) {
                if let Err(e) = proc_ctl.stop(Ingest) {
                    error!("{e}");
                };
            }

            if !is_running {
                proc_control.server_is_running.store(true, Ordering::SeqCst);
                playlist_init.store(true, Ordering::SeqCst);
                is_running = true;

                info!("Switch from {} to live ingest", config.processing.mode);

                if let Err(e) = proc_control.stop(Encoder) {
                    error!("{e}");
                }
            }

            log_line(&line, &config.logging.ffmpeg_level);
        }

        if proc_control.server_is_running.load(Ordering::SeqCst) {
            info!("Switch from live ingest to {}", config.processing.mode);
        }

        proc_control
            .server_is_running
            .store(false, Ordering::SeqCst);

        if let Err(e) = proc_control.wait(Ingest) {
            error!("{e}")
        }

        if proc_control.is_terminated.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}

/// HLS Writer
///
/// Write with single ffmpeg instance directly to a HLS playlist.
pub fn write_hls(
    config: &PlayoutConfig,
    player_control: PlayerControl,
    playout_stat: PlayoutStatus,
    proc_control: ProcessControl,
) {
    let config_clone = config.clone();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());
    let play_stat = playout_stat.clone();
    let play_stat2 = playout_stat.clone();
    let proc_control_c = proc_control.clone();

    let get_source = source_generator(
        config.clone(),
        &player_control,
        playout_stat,
        proc_control.is_terminated.clone(),
    );

    // spawn a thread for ffmpeg ingest server and create a channel for package sending
    if config.ingest.enable {
        thread::spawn(move || ingest_to_hls_server(config_clone, play_stat, proc_control_c));
    }

    for node in get_source {
        *player_control.current_media.lock().unwrap() = Some(node.clone());
        let ignore = config.logging.ignore_lines.clone();

        let mut cmd = match &node.cmd {
            Some(cmd) => cmd.clone(),
            None => break,
        };

        if !node.process.unwrap() {
            continue;
        }

        info!(
            "Play for <yellow>{}</>: <b><magenta>{}</></b>",
            sec_to_time(node.out - node.seek),
            node.source
        );

        if config.task.enable {
            if config.task.path.is_file() {
                let task_config = config.clone();
                let task_node = node.clone();
                let server_running = proc_control.server_is_running.load(Ordering::SeqCst);
                let stat = play_stat2.clone();

                thread::spawn(move || {
                    task_runner::run(task_config, task_node, stat, server_running)
                });
            } else {
                error!(
                    "<bright-blue>{:?}</> executable not exists!",
                    config.task.path
                );
            }
        }

        let mut enc_prefix = vec_strings!["-hide_banner", "-nostats", "-v", &ff_log_format];

        if let Some(encoder_input_cmd) = &ADVANCED_CONFIG.encoder.input_cmd {
            enc_prefix.append(&mut encoder_input_cmd.clone());
        }

        let mut read_rate = 1.0;

        if let Some(begin) = &node.begin {
            let (delta, _) = get_delta(config, begin);
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

        enc_prefix.append(&mut vec_strings!["-readrate", read_rate]);

        enc_prefix.append(&mut cmd);
        let enc_cmd = prepare_output_cmd(config, enc_prefix, &node.filter);

        debug!(
            "HLS writer CMD: <bright-blue>\"ffmpeg {}\"</>",
            enc_cmd.join(" ")
        );

        let mut dec_proc = match Command::new("ffmpeg")
            .args(enc_cmd)
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!("couldn't spawn ffmpeg process: {e}");
                panic!("couldn't spawn ffmpeg process: {e}")
            }
            Ok(proc) => proc,
        };

        let enc_err = BufReader::new(dec_proc.stderr.take().unwrap());
        *proc_control.decoder_term.lock().unwrap() = Some(dec_proc);

        if let Err(e) = stderr_reader(enc_err, ignore, Decoder, proc_control.clone()) {
            error!("{e:?}")
        };

        if let Err(e) = proc_control.wait(Decoder) {
            error!("{e}");
        }

        while proc_control.server_is_running.load(Ordering::SeqCst) {
            sleep(Duration::from_secs(1));
        }
    }

    sleep(Duration::from_secs(1));

    proc_control.stop_all();
}
