use std::{
    io::{BufRead, BufReader, Error, Read},
    process::{ChildStderr, Command, Stdio},
    sync::atomic::Ordering,
    sync::{Arc, Mutex},
    thread,
};

use crossbeam_channel::Sender;
use simplelog::*;

use ffplayout_lib::filter::ingest_filter::filter_cmd;
use ffplayout_lib::utils::{format_log_line, Ingest, PlayoutConfig, ProcessControl};
use ffplayout_lib::vec_strings;

pub fn log_line(line: String, level: &str) {
    if line.contains("[info]") && level.to_lowercase() == "info" {
        info!(
            "<bright black>[Server]</> {}",
            format_log_line(line, "info")
        )
    } else if line.contains("[warning]")
        && (level.to_lowercase() == "warning" || level.to_lowercase() == "info")
    {
        warn!(
            "<bright black>[Server]</> {}",
            format_log_line(line, "warning")
        )
    } else if line.contains("[error]")
        && !line.contains("Input/output error")
        && !line.contains("Broken pipe")
    {
        error!(
            "<bright black>[Server]</> {}",
            format_log_line(line, "error")
        );
    } else if line.contains("[fatal]") {
        error!(
            "<bright black>[Server]</> {}",
            format_log_line(line, "fatal")
        )
    }
}

fn server_monitor(
    level: &str,
    buffer: BufReader<ChildStderr>,
    mut proc_ctl: ProcessControl,
) -> Result<(), Error> {
    for line in buffer.lines() {
        let line = line?;

        if line.contains("rtmp") && line.contains("Unexpected stream") {
            if let Err(e) = proc_ctl.kill(Ingest) {
                error!("{e}");
            };

            warn!(
                "<bright black>[Server]</> {}",
                format_log_line(line.clone(), "error")
            );
        }

        if line.contains("Address already in use") {
            proc_ctl.kill_all();
        }

        log_line(line, level);
    }

    Ok(())
}

/// ffmpeg Ingest Server
///
/// Start ffmpeg in listen mode, and wait for input.
pub fn ingest_server(
    config: PlayoutConfig,
    ingest_sender: Sender<(usize, [u8; 65088])>,
    mut proc_control: ProcessControl,
) -> Result<(), Error> {
    let mut buffer: [u8; 65088] = [0; 65088];
    let mut server_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let stream_input = config.ingest.input_cmd.clone().unwrap();

    server_cmd.append(&mut stream_input.clone());
    server_cmd.append(&mut filter_cmd(&config, &Arc::new(Mutex::new(vec![]))));
    server_cmd.append(&mut config.processing.settings.unwrap());

    let mut is_running;

    if let Some(url) = stream_input.iter().find(|s| s.contains("://")) {
        info!("Start ingest server, listening on: <b><magenta>{url}</></b>",);
    };

    debug!(
        "Server CMD: <bright-blue>\"ffmpeg {}\"</>",
        server_cmd.join(" ")
    );

    while !proc_control.is_terminated.load(Ordering::SeqCst) {
        let proc_ctl = proc_control.clone();
        let level = config.logging.ffmpeg_level.clone();
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
            thread::spawn(move || server_monitor(&level, server_err, proc_ctl));

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
