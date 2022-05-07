use std::{
    io::{BufReader, Error, Read},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    thread,
};

use crossbeam_channel::Sender;
use simplelog::*;

use crate::filter::ingest_filter::filter_cmd;
use crate::utils::{stderr_reader, GlobalConfig, Ingest, ProcessControl};

/// ffmpeg Ingest Server
///
/// Start ffmpeg in listen mode, and wait for input.
pub fn ingest_server(
    log_format: String,
    ingest_sender: Sender<(usize, [u8; 65088])>,
    mut proc_control: ProcessControl,
) -> Result<(), Error> {
    let config = GlobalConfig::global();
    let mut buffer: [u8; 65088] = [0; 65088];
    let filter_list = filter_cmd();

    let mut server_cmd = vec!["-hide_banner", "-nostats", "-v", log_format.as_str()];
    let stream_input = config.ingest.input_cmd.clone().unwrap();
    let stream_settings = config.processing.settings.clone().unwrap();

    server_cmd.append(&mut stream_input.iter().map(String::as_str).collect());
    server_cmd.append(&mut filter_list.iter().map(String::as_str).collect());
    server_cmd.append(&mut stream_settings.iter().map(String::as_str).collect());

    let mut is_running;

    info!(
        "Start ingest server, listening on: <b><magenta>{}</></b>",
        stream_input.last().unwrap()
    );

    debug!(
        "Server CMD: <bright-blue>\"ffmpeg {}\"</>",
        server_cmd.join(" ")
    );

    'ingest_iter: loop {
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
        let error_reader_thread = thread::spawn(move || stderr_reader(server_err, "Server"));

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
                    break 'ingest_iter;
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

        if proc_control.is_terminated.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}
