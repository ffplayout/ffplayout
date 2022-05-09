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
        -hls_segment_filename /var/www/html/live/stream-%09d.ts /var/www/html/live/stream.m3u8

*/

use std::{
    io::{BufRead, BufReader, Error},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    thread::{self, sleep},
    time::Duration,
};

use simplelog::*;

use crate::filter::ingest_filter::filter_cmd;
use crate::input::source_generator;
use crate::utils::{
    sec_to_time, stderr_reader, Decoder, GlobalConfig, Ingest, PlayerControl, PlayoutStatus,
    ProcessControl,
};
use crate::vec_strings;

fn format_line(line: String, level: &str) -> String {
    line.replace(&format!("[{level: >5}] "), "")
}

/// Ingest Server for HLS
fn ingest_to_hls_server(
    playout_stat: PlayoutStatus,
    mut proc_control: ProcessControl,
) -> Result<(), Error> {
    let config = GlobalConfig::global();
    let playlist_init = playout_stat.list_init;

    let mut server_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();

    server_cmd.append(&mut stream_input.clone());
    server_cmd.append(&mut filter_cmd());
    server_cmd.append(&mut config.out.clone().output_cmd.unwrap());

    let mut is_running;

    if let Some(url) = stream_input.iter().find(|s| s.contains("://")) {
        info!("Start ingest server, listening on: <b><magenta>{url}</></b>",);
    };

    debug!(
        "Server CMD: <bright-blue>\"ffmpeg {}\"</>",
        server_cmd.join(" ")
    );

    loop {
        let mut server_proc = match Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!("couldn't spawn ingest server: {e}");
                panic!("couldn't spawn ingest server: {e}")
            }
            Ok(proc) => proc,
        };

        let server_err = BufReader::new(server_proc.stderr.take().unwrap());
        *proc_control.server_term.lock().unwrap() = Some(server_proc);
        is_running = false;

        for line in server_err.lines() {
            let line = line?;

            if !is_running {
                proc_control.server_is_running.store(true, Ordering::SeqCst);
                playlist_init.store(true, Ordering::SeqCst);
                is_running = true;

                info!("Switch from {} to live ingest", config.processing.mode);

                if let Err(e) = proc_control.kill(Decoder) {
                    error!("{e}");
                }
            }

            if line.contains("[error]")
                && !line.contains("Input/output error")
                && !line.contains("Broken pipe")
            {
                error!(
                    "<bright black>[server]</> {}",
                    format_line(line.clone(), "error")
                );
            }
        }

        info!("Switch from live ingest to {}", config.processing.mode);

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
    play_control: PlayerControl,
    playout_stat: PlayoutStatus,
    mut proc_control: ProcessControl,
) {
    let config = GlobalConfig::global();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());
    let play_stat = playout_stat.clone();
    let proc_control_c = proc_control.clone();

    let get_source = source_generator(
        config.clone(),
        play_control.current_list.clone(),
        play_control.index.clone(),
        playout_stat,
        proc_control.is_terminated.clone(),
    );

    // spawn a thread for ffmpeg ingest server and create a channel for package sending
    if config.ingest.enable {
        thread::spawn(move || ingest_to_hls_server(play_stat, proc_control_c));
    }

    for node in get_source {
        *play_control.current_media.lock().unwrap() = Some(node.clone());

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

        let mut filter = node.filter.unwrap();
        let mut dec_cmd = vec_strings!["-hide_banner", "-nostats", "-v", &ff_log_format];
        dec_cmd.append(&mut cmd);

        if filter.len() > 1 {
            dec_cmd.append(&mut filter);
        }

        dec_cmd.append(&mut config.out.clone().output_cmd.unwrap());

        debug!(
            "HLS writer CMD: <bright-blue>\"ffmpeg {}\"</>",
            dec_cmd.join(" ")
        );

        let mut dec_proc = match Command::new("ffmpeg")
            .args(dec_cmd)
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!("couldn't spawn decoder process: {e}");
                panic!("couldn't spawn decoder process: {e}")
            }
            Ok(proc) => proc,
        };

        let dec_err = BufReader::new(dec_proc.stderr.take().unwrap());
        *proc_control.decoder_term.lock().unwrap() = Some(dec_proc);

        if let Err(e) = stderr_reader(dec_err, "Writer") {
            error!("{e:?}")
        };

        if let Err(e) = proc_control.wait(Decoder) {
            error!("{e}");
        }

        while proc_control.server_is_running.load(Ordering::SeqCst) {
            sleep(Duration::from_secs(1));
        }
    }
}
