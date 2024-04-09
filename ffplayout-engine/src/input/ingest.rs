use std::{
    io::{BufRead, BufReader, Error, Read},
    process::{exit, ChildStderr, Command, Stdio},
    sync::atomic::Ordering,
    thread,
};

use crossbeam_channel::Sender;
use simplelog::*;

use crate::utils::{log_line, valid_stream};
use ffplayout_lib::{
    utils::{
        controller::ProcessUnit::*, test_tcp_port, Media, PlayoutConfig, ProcessControl,
        FFMPEG_IGNORE_ERRORS, FFMPEG_UNRECOVERABLE_ERRORS,
    },
    vec_strings, ADVANCED_CONFIG,
};

fn server_monitor(
    level: &str,
    ignore: Vec<String>,
    buffer: BufReader<ChildStderr>,
    proc_ctl: ProcessControl,
) -> Result<(), Error> {
    for line in buffer.lines() {
        let line = line?;

        if !FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i))
            && !ignore.iter().any(|i| line.contains(i))
        {
            log_line(&line, level);
        }

        if line.contains("rtmp") && line.contains("Unexpected stream") && !valid_stream(&line) {
            if let Err(e) = proc_ctl.stop(Ingest) {
                error!("{e}");
            };
        }

        if FFMPEG_UNRECOVERABLE_ERRORS
            .iter()
            .any(|i| line.contains(*i))
        {
            proc_ctl.stop_all();
        }
    }

    Ok(())
}

/// ffmpeg Ingest Server
///
/// Start ffmpeg in listen mode, and wait for input.
pub fn ingest_server(
    config: PlayoutConfig,
    ingest_sender: Sender<(usize, [u8; 65088])>,
    proc_control: ProcessControl,
) -> Result<(), Error> {
    let mut buffer: [u8; 65088] = [0; 65088];
    let mut server_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let mut dummy_media = Media::new(0, "Live Stream", false);
    dummy_media.unit = Ingest;
    dummy_media.add_filter(&config, &None);

    if let Some(ingest_input_cmd) = &ADVANCED_CONFIG.ingest.input_cmd {
        server_cmd.append(&mut ingest_input_cmd.clone());
    }

    server_cmd.append(&mut stream_input.clone());

    if let Some(mut filter) = dummy_media.filter {
        server_cmd.append(&mut filter.cmd());
        server_cmd.append(&mut filter.map());
    }

    if let Some(mut cmd) = config.processing.cmd {
        server_cmd.append(&mut cmd);
    }

    let mut is_running;

    if let Some(url) = stream_input.iter().find(|s| s.contains("://")) {
        if !test_tcp_port(url) {
            proc_control.stop_all();
            exit(1);
        }

        info!("Start ingest server, listening on: <b><magenta>{url}</></b>",);
    };

    debug!(
        "Server CMD: <bright-blue>\"ffmpeg {}\"</>",
        server_cmd.join(" ")
    );

    while !proc_control.is_terminated.load(Ordering::SeqCst) {
        let proc_ctl = proc_control.clone();
        let level = config.logging.ingest_level.clone().unwrap();
        let ignore = config.logging.ignore_lines.clone();
        let mut server_proc = match Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!("couldn't spawn ingest server: {e}");
                panic!("couldn't spawn ingest server: {e}")
            }
            Ok(proc) => proc,
        };
        let mut ingest_reader = BufReader::new(server_proc.stdout.take().unwrap());
        let server_err = BufReader::new(server_proc.stderr.take().unwrap());
        let error_reader_thread =
            thread::spawn(move || server_monitor(&level, ignore, server_err, proc_ctl));

        *proc_control.server_term.lock().unwrap() = Some(server_proc);
        is_running = false;

        loop {
            let bytes_len = match ingest_reader.read(&mut buffer[..]) {
                Ok(length) => length,
                Err(e) => {
                    debug!("Ingest server read {e:?}");
                    break;
                }
            };

            if !is_running {
                proc_control.server_is_running.store(true, Ordering::SeqCst);
                is_running = true;
            }

            if bytes_len > 0 {
                if let Err(e) = ingest_sender.send((bytes_len, buffer)) {
                    error!("Ingest server write error: {e:?}");

                    proc_control.is_terminated.store(true, Ordering::SeqCst);
                    break;
                }
            } else {
                break;
            }
        }

        drop(ingest_reader);
        proc_control
            .server_is_running
            .store(false, Ordering::SeqCst);

        if let Err(e) = proc_control.wait(Ingest) {
            error!("{e}")
        }

        if let Err(e) = error_reader_thread.join() {
            error!("{e:?}");
        };
    }

    Ok(())
}
