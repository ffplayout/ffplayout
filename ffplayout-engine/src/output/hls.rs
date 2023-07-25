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
use crate::utils::{log_line, prepare_output_cmd, valid_stream};
use ffplayout_lib::{
    utils::{
        controller::ProcessUnit::*, sec_to_time, stderr_reader, test_tcp_port, Media,
        PlayerControl, PlayoutConfig, PlayoutStatus, ProcessControl,
    },
    vec_strings,
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
    server_prefix.append(&mut stream_input.clone());
    let mut dummy_media = Media::new(0, "Live Stream", false);
    dummy_media.unit = Ingest;

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

        let mut cmd = match node.cmd {
            Some(cmd) => cmd,
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

        let mut enc_prefix = vec_strings!["-hide_banner", "-nostats", "-v", &ff_log_format];

        if config.processing.copy_video
            || (!config.processing.audio_only && config.processing.copy_audio)
        {
            enc_prefix.push("-re".to_string());
        }
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

        if let Err(e) = stderr_reader(enc_err, Decoder, proc_control.clone()) {
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
