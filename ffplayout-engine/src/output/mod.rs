use std::{
    io::{prelude::*, BufReader, BufWriter, Read},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    thread::{self, sleep},
    time::Duration,
};

use crossbeam_channel::bounded;
use simplelog::*;

mod desktop;
mod hls;
mod null;
mod stream;

pub use hls::write_hls;

use crate::input::{ingest_server, source_generator};
use ffplayout_lib::utils::{
    sec_to_time, stderr_reader, Decoder, OutputMode::*, PlayerControl, PlayoutConfig,
    PlayoutStatus, ProcessControl,
};
use ffplayout_lib::vec_strings;

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
    config: &PlayoutConfig,
    play_control: PlayerControl,
    playout_stat: PlayoutStatus,
    mut proc_control: ProcessControl,
) {
    let config_clone = config.clone();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());
    let mut buffer = [0; 65088];
    let mut live_on = false;
    let playlist_init = playout_stat.list_init.clone();

    // get source iterator
    let get_source = source_generator(
        config.clone(),
        play_control.current_list.clone(),
        play_control.index.clone(),
        playout_stat,
        proc_control.is_terminated.clone(),
    );

    // get ffmpeg output instance
    let mut enc_proc = match config.out.mode {
        Desktop => desktop::output(config, &ff_log_format),
        Null => null::output(config, &ff_log_format),
        Stream => stream::output(config, &ff_log_format),
        _ => panic!("Output mode doesn't exists!"),
    };

    let mut enc_writer = BufWriter::new(enc_proc.stdin.take().unwrap());
    let enc_err = BufReader::new(enc_proc.stderr.take().unwrap());

    *proc_control.encoder_term.lock().unwrap() = Some(enc_proc);
    let enc_p_ctl = proc_control.clone();

    // spawn a thread to log ffmpeg output error messages
    let error_encoder_thread = thread::spawn(move || stderr_reader(enc_err, "Encoder", enc_p_ctl));

    let proc_control_c = proc_control.clone();
    let mut ingest_receiver = None;

    // spawn a thread for ffmpeg ingest server and create a channel for package sending
    if config.ingest.enable {
        let (ingest_sender, rx) = bounded(96);
        ingest_receiver = Some(rx);
        thread::spawn(move || ingest_server(config_clone, ingest_sender, proc_control_c));
    }

    'source_iter: for node in get_source {
        *play_control.current_media.lock().unwrap() = Some(node.clone());

        let mut cmd = match node.cmd {
            Some(cmd) => cmd,
            None => break,
        };

        if !node.process.unwrap() {
            continue;
        }

        info!(
            "Play for <yellow>{}</>: <b><magenta>{}  {}</></b>",
            sec_to_time(node.out - node.seek),
            node.source,
            node.audio
        );

        let mut dec_cmd = vec_strings!["-hide_banner", "-nostats", "-v", &ff_log_format];
        dec_cmd.append(&mut cmd);

        if let Some(mut filter) = node.filter {
            dec_cmd.append(&mut filter.cmd);
        }

        dec_cmd.append(&mut config.processing.clone().settings.unwrap());

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
                error!("couldn't spawn decoder process: {}", e);
                panic!("couldn't spawn decoder process: {}", e)
            }
        };

        let mut dec_reader = BufReader::new(dec_proc.stdout.take().unwrap());
        let dec_err = BufReader::new(dec_proc.stderr.take().unwrap());

        *proc_control.decoder_term.lock().unwrap() = Some(dec_proc);
        let dec_p_ctl = proc_control.clone();

        let error_decoder_thread =
            thread::spawn(move || stderr_reader(dec_err, "Decoder", dec_p_ctl));

        loop {
            // when server is running, read from channel
            if proc_control.server_is_running.load(Ordering::SeqCst) {
                if !live_on {
                    info!("Switch from {} to live ingest", config.processing.mode);

                    if let Err(e) = enc_writer.flush() {
                        error!("Encoder error: {e}")
                    }

                    if let Err(e) = proc_control.kill(Decoder) {
                        error!("{e}")
                    }

                    live_on = true;
                    playlist_init.store(true, Ordering::SeqCst);
                }

                for rx in ingest_receiver.as_ref().unwrap().try_iter() {
                    if let Err(e) = enc_writer.write(&rx.1[..rx.0]) {
                        error!("Encoder write error: {:?}", e);

                        break 'source_iter;
                    };
                }
            // read from decoder instance
            } else {
                if live_on {
                    info!("Switch from live ingest to {}", config.processing.mode);

                    if let Err(e) = enc_writer.flush() {
                        error!("Encoder error: {e}")
                    }

                    live_on = false;
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
                        error!("Encoder write error: {e:?}");

                        break 'source_iter;
                    };
                } else {
                    break;
                }
            }
        }

        if let Err(e) = proc_control.wait(Decoder) {
            error!("{e}")
        }

        if let Err(e) = error_decoder_thread.join() {
            error!("{e:?}");
        };
    }

    sleep(Duration::from_secs(1));

    proc_control.kill_all();

    if let Err(e) = error_encoder_thread.join() {
        error!("{e:?}");
    };
}
