use std::{
    io::{prelude::*, BufReader, BufWriter, Read},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    thread::{self, sleep},
    time::Duration,
};

use crossbeam_channel::bounded;
use log::*;
use sqlx::{Pool, Sqlite};

mod desktop;
mod hls;
mod null;
mod stream;

pub use hls::write_hls;

use crate::player::{
    controller::{ChannelManager, PlayerControl, PlayoutStatus, ProcessUnit::*},
    input::{ingest_server, source_generator},
    utils::{sec_to_time, stderr_reader},
};
use crate::utils::{config::OutputMode::*, errors::ProcessError, task_runner};
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
pub fn player(
    channel_mgr: ChannelManager,
    db_pool: Pool<Sqlite>,
    play_control: &PlayerControl,
    playout_stat: PlayoutStatus,
) -> Result<(), ProcessError> {
    let config = channel_mgr.config.lock()?.clone();
    let config_clone = config.clone();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());
    let ignore_enc = config.logging.ignore_lines.clone();
    let mut buffer = [0; 65088];
    let mut live_on = false;
    let playlist_init = playout_stat.list_init.clone();

    let is_terminated = channel_mgr.is_terminated.clone();
    let ingest_is_running = channel_mgr.ingest_is_running.clone();

    // get source iterator
    let node_sources = source_generator(
        config.clone(),
        db_pool,
        play_control,
        playout_stat,
        is_terminated.clone(),
    );

    // get ffmpeg output instance
    let mut enc_proc = match config.output.mode {
        Desktop => desktop::output(&config, &ff_log_format),
        Null => null::output(&config, &ff_log_format),
        Stream => stream::output(&config, &ff_log_format),
        _ => panic!("Output mode doesn't exists!"),
    };

    let mut enc_writer = BufWriter::new(enc_proc.stdin.take().unwrap());
    let enc_err = BufReader::new(enc_proc.stderr.take().unwrap());

    *channel_mgr.encoder.lock().unwrap() = Some(enc_proc);
    let enc_p_ctl = channel_mgr.clone();

    // spawn a thread to log ffmpeg output error messages
    let error_encoder_thread =
        thread::spawn(move || stderr_reader(enc_err, ignore_enc, Encoder, enc_p_ctl));

    let channel_mgr_2 = channel_mgr.clone();
    let mut ingest_receiver = None;

    // spawn a thread for ffmpeg ingest server and create a channel for package sending
    if config.ingest.enable {
        let (ingest_sender, rx) = bounded(96);
        ingest_receiver = Some(rx);
        thread::spawn(move || ingest_server(config_clone, ingest_sender, channel_mgr_2));
    }

    'source_iter: for node in node_sources {
        *play_control.current_media.lock().unwrap() = Some(node.clone());
        let ignore_dec = config.logging.ignore_lines.clone();

        if is_terminated.load(Ordering::SeqCst) {
            debug!("Playout is terminated, break out from source loop");
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
                play_control.current_list.lock().unwrap().len()
            )
        } else {
            String::new()
        };

        info!(
            "Play for <yellow>{}</>{c_index}: <b><magenta>{}  {}</></b>",
            sec_to_time(node.out - node.seek),
            node.source,
            node.audio
        );

        if config.task.enable {
            if config.task.path.is_file() {
                let channel_mgr_3 = channel_mgr.clone();

                thread::spawn(move || task_runner::run(channel_mgr_3));
            } else {
                error!(
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

        if let Some(mut cmd) = config.processing.cmd.clone() {
            dec_cmd.append(&mut cmd);
        }

        debug!(
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
                error!("couldn't spawn decoder process: {e}");
                panic!("couldn't spawn decoder process: {e}")
            }
        };

        let mut dec_reader = BufReader::new(dec_proc.stdout.take().unwrap());
        let dec_err = BufReader::new(dec_proc.stderr.take().unwrap());

        *channel_mgr.clone().decoder.lock().unwrap() = Some(dec_proc);
        let channel_mgr_c = channel_mgr.clone();

        let error_decoder_thread =
            thread::spawn(move || stderr_reader(dec_err, ignore_dec, Decoder, channel_mgr_c));

        loop {
            // when server is running, read from it
            if ingest_is_running.load(Ordering::SeqCst) {
                if !live_on {
                    info!("Switch from {} to live ingest", config.processing.mode);

                    if let Err(e) = channel_mgr.stop(Decoder) {
                        error!("{e}")
                    }

                    live_on = true;
                    playlist_init.store(true, Ordering::SeqCst);
                }

                for rx in ingest_receiver.as_ref().unwrap().try_iter() {
                    if let Err(e) = enc_writer.write(&rx.1[..rx.0]) {
                        error!("Error from Ingest: {:?}", e);

                        break 'source_iter;
                    };
                }
            // read from decoder instance
            } else {
                if live_on {
                    info!("Switch from live ingest to {}", config.processing.mode);

                    live_on = false;
                    break;
                }

                let dec_bytes_len = match dec_reader.read(&mut buffer[..]) {
                    Ok(length) => length,
                    Err(e) => {
                        error!("Reading error from decoder: {e:?}");

                        break 'source_iter;
                    }
                };

                if dec_bytes_len > 0 {
                    if let Err(e) = enc_writer.write(&buffer[..dec_bytes_len]) {
                        error!("Encoder write error: {}", e.kind());

                        break 'source_iter;
                    };
                } else {
                    break;
                }
            }
        }

        if let Err(e) = channel_mgr.wait(Decoder) {
            error!("{e}")
        }

        if let Err(e) = error_decoder_thread.join() {
            error!("{e:?}");
        };
    }

    trace!("Out of source loop");

    sleep(Duration::from_secs(1));

    channel_mgr.stop_all();

    if let Err(e) = error_encoder_thread.join() {
        error!("{e:?}");
    };

    Ok(())
}
