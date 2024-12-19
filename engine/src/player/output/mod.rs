use std::{
    io::{prelude::*, BufReader, BufWriter, Read},
    process::{Command, Stdio},
    sync::{atomic::Ordering, mpsc::sync_channel},
    thread::{self, sleep},
    time::{Duration, SystemTime},
};

use log::*;

mod desktop;
mod hls;
mod null;
mod stream;

pub use hls::write_hls;

use crate::player::{
    controller::{ChannelManager, ProcessUnit::*},
    input::{ingest_server, source_generator},
    utils::{sec_to_time, stderr_reader},
};
use crate::utils::{config::OutputMode::*, errors::ProcessError, logging::Target, task_runner};
use crate::vec_strings;

/// Player
///
/// Here we create the input file loop, from playlist, or folder source.
/// Then we read the stdout from the reader ffmpeg instance
/// and write it to the stdin from the streamer ffmpeg instance.
/// If it is configured we also fire up a ffmpeg ingest server instance,
/// for getting live feeds.
/// When a live ingest arrive, it stops the current playing and switch to the live source.
/// When ingest stops, it switch back to playlist/folder mode.
pub fn player(manager: ChannelManager) -> Result<(), ProcessError> {
    let config = manager.config.lock()?.clone();
    let id = config.general.channel_id;
    let config_clone = config.clone();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());
    let ignore_enc = config.logging.ignore_lines.clone();
    let mut buffer = [0; 65088];
    let mut live_on = false;
    let playlist_init = manager.list_init.clone();

    let is_terminated = manager.is_terminated.clone();
    let ingest_is_running = manager.ingest_is_running.clone();

    // get source iterator
    let node_sources = source_generator(manager.clone());

    // get ffmpeg output instance
    let mut enc_proc = match config.output.mode {
        Desktop => desktop::output(&config, &ff_log_format),
        Null => null::output(&config, &ff_log_format),
        Stream => stream::output(&config, &ff_log_format),
        _ => panic!("Output mode doesn't exists!"),
    };

    let mut enc_writer = BufWriter::new(enc_proc.stdin.take().unwrap());
    let enc_err = BufReader::new(enc_proc.stderr.take().unwrap());

    *manager.encoder.lock().unwrap() = Some(enc_proc);
    let enc_p_ctl = manager.clone();

    // spawn a thread to log ffmpeg output error messages
    let error_encoder_thread =
        thread::spawn(move || stderr_reader(enc_err, ignore_enc, Encoder, enc_p_ctl));

    let channel_mgr_2 = manager.clone();
    let mut ingest_receiver = None;

    // spawn a thread for ffmpeg ingest server and create a channel for package sending
    if config.ingest.enable {
        let (ingest_sender, rx) = sync_channel(96);
        ingest_receiver = Some(rx);
        thread::spawn(move || ingest_server(config_clone, ingest_sender, channel_mgr_2));
    }

    drop(config);

    let mut error_count = 0;

    'source_iter: for node in node_sources {
        let config = manager.config.lock()?.clone();

        *manager.current_media.lock().unwrap() = Some(node.clone());
        let ignore_dec = config.logging.ignore_lines.clone();
        let timer = SystemTime::now();

        if is_terminated.load(Ordering::SeqCst) {
            debug!(target: Target::file_mail(), channel = id; "Playout is terminated, break out from source loop");
            break;
        }

        trace!("Decoder CMD: {:?}", node.cmd);

        let mut cmd = match &node.cmd {
            Some(cmd) => cmd.clone(),
            None => break,
        };

        if !node.process.unwrap() {
            // process true/false differs from node.cmd = None in that way,
            // that source is valid but to show for playing,
            // so better skip it and jump to the next one.
            continue;
        }

        let c_index = if cfg!(debug_assertions) {
            format!(
                " ({}/{})",
                node.index.unwrap() + 1,
                manager.current_list.lock().unwrap().len()
            )
        } else {
            String::new()
        };

        info!(target: Target::file_mail(), channel = id;
            "Play for <yellow>{}</>{c_index}: <b><magenta>{}  {}</></b>",
            sec_to_time(node.out - node.seek),
            node.source,
            node.audio
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

        let mut dec_cmd = vec_strings!["-hide_banner", "-nostats", "-v", &ff_log_format];

        if let Some(decoder_input_cmd) = &config.advanced.decoder.input_cmd {
            dec_cmd.append(&mut decoder_input_cmd.clone());
        }

        dec_cmd.append(&mut cmd);

        if let Some(mut filter) = node.filter {
            dec_cmd.append(&mut filter.cmd());
            dec_cmd.append(&mut filter.map());
        }

        if config.processing.vtt_enable && dec_cmd.iter().any(|s| s.ends_with(".vtt")) {
            let i = dec_cmd
                .iter()
                .filter(|&n| n == "-i")
                .count()
                .saturating_sub(1);

            dec_cmd.append(&mut vec_strings!("-map", format!("{i}:s"), "-c:s", "copy"));
        }

        if let Some(mut cmd) = config.processing.cmd.clone() {
            dec_cmd.append(&mut cmd);
        }

        debug!(target: Target::file_mail(), channel = id;
            "Decoder CMD: <bright-blue>\"ffmpeg {}\"</>",
            dec_cmd.join(" ")
        );

        // create ffmpeg decoder instance, for reading the input files
        let mut dec_proc = match Command::new("ffmpeg")
            .args(dec_cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(proc) => proc,
            Err(e) => {
                error!(target: Target::file_mail(), channel = id; "couldn't spawn decoder process: {e}");
                panic!("couldn't spawn decoder process: {e}")
            }
        };

        let mut dec_reader = BufReader::new(dec_proc.stdout.take().unwrap());
        let dec_err = BufReader::new(dec_proc.stderr.take().unwrap());

        *manager.clone().decoder.lock().unwrap() = Some(dec_proc);
        let channel_mgr_c = manager.clone();

        let error_decoder_thread =
            thread::spawn(move || stderr_reader(dec_err, ignore_dec, Decoder, channel_mgr_c));

        loop {
            // when server is running, read from it
            if ingest_is_running.load(Ordering::SeqCst) {
                if !live_on {
                    info!(target: Target::file_mail(), channel = id; "Switch from {} to live ingest", config.processing.mode);

                    if let Err(e) = manager.stop(Decoder) {
                        error!(target: Target::file_mail(), channel = id; "{e}");
                    }

                    live_on = true;
                    playlist_init.store(true, Ordering::SeqCst);
                }

                for rx in ingest_receiver.as_ref().unwrap().try_iter() {
                    if let Err(e) = enc_writer.write(&rx.1[..rx.0]) {
                        error!(target: Target::file_mail(), channel = id; "Error from Ingest: {:?}", e);

                        break 'source_iter;
                    };
                }
            // read from decoder instance
            } else {
                if live_on {
                    info!(target: Target::file_mail(), channel = id; "Switch from live ingest to {}", config.processing.mode);

                    live_on = false;
                    break;
                }

                let dec_bytes_len = match dec_reader.read(&mut buffer[..]) {
                    Ok(length) => length,
                    Err(e) => {
                        error!(target: Target::file_mail(), channel = id; "Reading error from decoder: {e:?}");

                        break 'source_iter;
                    }
                };

                if dec_bytes_len > 0 {
                    if let Err(e) = enc_writer.write(&buffer[..dec_bytes_len]) {
                        error!(target: Target::file_mail(), channel = id; "Encoder write error: {}", e.kind());

                        break 'source_iter;
                    };
                } else {
                    break;
                }
            }
        }

        if let Err(e) = manager.wait(Decoder) {
            error!(target: Target::file_mail(), channel = id; "{e}");
        }

        if let Err(e) = error_decoder_thread.join() {
            error!(target: Target::file_mail(), channel = id; "{e:?}");
        };

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

    trace!("Out of source loop");

    sleep(Duration::from_secs(1));

    manager.stop_all();

    if let Err(e) = error_encoder_thread.join() {
        error!(target: Target::file_mail(), channel = id; "{e:?}");
    };

    Ok(())
}
