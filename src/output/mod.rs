use std::{
    io::{prelude::*, BufReader, BufWriter, Read},
    process::{Command, Stdio},
    thread::sleep,
    time::Duration,
};

use crossbeam_channel::bounded;
use simplelog::*;
use tokio::runtime::Handle;

mod desktop;
mod hls;
mod stream;

pub use hls::write_hls;

use crate::input::{ingest_server, source_generator};
use crate::utils::{
    sec_to_time, stderr_reader, Decoder, Encoder, GlobalConfig, PlayerControl, PlayoutStatus,
    ProcessControl,
};

pub fn player(
    rt_handle: &Handle,
    play_control: PlayerControl,
    playout_stat: PlayoutStatus,
    mut proc_control: ProcessControl,
) {
    let config = GlobalConfig::global();
    let dec_settings = config.processing.clone().settings.unwrap();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());
    let mut buffer = [0; 65088];
    let mut live_on = false;
    let playlist_init = playout_stat.list_init.clone();

    let get_source = source_generator(
        rt_handle,
        config.clone(),
        play_control.current_list.clone(),
        play_control.index.clone(),
        playout_stat,
        proc_control.is_terminated.clone(),
    );

    let mut enc_proc = match config.out.mode.as_str() {
        "desktop" => desktop::output(&ff_log_format),
        "stream" => stream::output(&ff_log_format),
        _ => panic!("Output mode doesn't exists!"),
    };

    let mut enc_writer = BufWriter::new(enc_proc.stdin.take().unwrap());
    rt_handle.spawn(stderr_reader(enc_proc.stderr.take().unwrap(), "Encoder"));
    *proc_control.decoder_term.lock().unwrap() = Some(enc_proc);

    let (ingest_sender, ingest_receiver) = bounded(96);

    if config.ingest.enable {
        rt_handle.spawn(ingest_server(
            ff_log_format.clone(),
            ingest_sender.clone(),
            rt_handle.clone(),
            proc_control.clone(),
        ));
    }

    'source_iter: for node in get_source {
        *play_control.current_media.lock().unwrap() = Some(node.clone());

        let cmd = match node.cmd {
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

        let filter = node.filter.unwrap();
        let mut dec_cmd = vec!["-hide_banner", "-nostats", "-v", ff_log_format.as_str()];
        dec_cmd.append(&mut cmd.iter().map(String::as_str).collect());

        if filter.len() > 1 {
            dec_cmd.append(&mut filter.iter().map(String::as_str).collect());
        }

        dec_cmd.append(&mut dec_settings.iter().map(String::as_str).collect());

        debug!(
            "Decoder CMD: <bright-blue>\"ffmpeg {}\"</>",
            dec_cmd.join(" ")
        );

        let mut dec_proc = match Command::new("ffmpeg")
            .args(dec_cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!("couldn't spawn decoder process: {}", e);
                panic!("couldn't spawn decoder process: {}", e)
            }
            Ok(proc) => proc,
        };

        let mut dec_reader = BufReader::new(dec_proc.stdout.take().unwrap());
        rt_handle.spawn(stderr_reader(dec_proc.stderr.take().unwrap(), "Decoder"));
        *proc_control.decoder_term.lock().unwrap() = Some(dec_proc);

        loop {
            if *proc_control.server_is_running.lock().unwrap() {
                if !live_on {
                    info!("Switch from {} to live ingest", config.processing.mode);

                    if let Err(e) = enc_writer.flush() {
                        error!("Encoder error: {e}")
                    }

                    if let Err(e) = proc_control.kill(Decoder) {
                        error!("{e}")
                    }

                    live_on = true;
                    *playlist_init.lock().unwrap() = true;
                }

                for rx in ingest_receiver.try_iter() {
                    if let Err(e) = enc_writer.write(&rx.1[..rx.0]) {
                        error!("Encoder write error: {:?}", e);

                        break 'source_iter;
                    };
                }
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
    }

    sleep(Duration::from_secs(1));

    if let Err(e) = proc_control.kill(Encoder) {
        error!("{e}")
    }
}
