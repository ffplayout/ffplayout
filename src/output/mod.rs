use notify::{watcher, RecursiveMode, Watcher};
use std::{
    io::{prelude::*, BufReader, BufWriter, Read},
    path::Path,
    process,
    process::{Command, Stdio},
    sync::{
        mpsc::{channel, sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread::sleep,
    time::Duration,
};

use process_control::ChildExt;
use simplelog::*;
use tokio::runtime::Handle;

mod desktop;
mod hls;
mod stream;

pub use hls::write_hls;

use crate::input::{file_worker, ingest_server, CurrentProgram, Source};
use crate::utils::{
    sec_to_time, stderr_reader, GlobalConfig, Media, PlayerControl, PlayoutStatus, ProcessControl,
};

pub fn source_generator(
    rt_handle: &Handle,
    config: GlobalConfig,
    current_list: Arc<Mutex<Vec<Media>>>,
    index: Arc<Mutex<usize>>,
    playout_stat: PlayoutStatus,
    is_terminated: Arc<Mutex<bool>>,
) -> (Box<dyn Iterator<Item = Media>>, Arc<Mutex<bool>>) {
    let mut init_playlist: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    let get_source = match config.processing.clone().mode.as_str() {
        "folder" => {
            let path = config.storage.path.clone();
            if !Path::new(&path).exists() {
                error!("Folder path not exists: '{path}'");
                process::exit(0x0100);
            }

            info!("Playout in folder mode.");

            let folder_source = Source::new(current_list, index, playout_stat);

            let (sender, receiver) = channel();
            let mut watchman = watcher(sender, Duration::from_secs(2)).unwrap();
            watchman
                .watch(path.clone(), RecursiveMode::Recursive)
                .unwrap();

            debug!("Monitor folder: <b><magenta>{}</></b>", path);

            rt_handle.spawn(file_worker(receiver, folder_source.nodes.clone()));

            Box::new(folder_source) as Box<dyn Iterator<Item = Media>>
        }
        "playlist" => {
            info!("Playout in playlist mode");
            let program = CurrentProgram::new(
                rt_handle.clone(),
                playout_stat,
                is_terminated.clone(),
                current_list,
                index,
            );
            init_playlist = program.init.clone();

            Box::new(program) as Box<dyn Iterator<Item = Media>>
        }
        _ => {
            error!("Process Mode not exists!");
            process::exit(0x0100);
        }
    };

    (get_source, init_playlist)
}

pub fn player(
    rt_handle: &Handle,
    play_control: PlayerControl,
    playout_stat: PlayoutStatus,
    proc_control: ProcessControl,
) {
    let config = GlobalConfig::global();
    let dec_settings = config.processing.clone().settings.unwrap();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());

    let server_is_running: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let mut buffer: [u8; 65088] = [0; 65088];
    let mut live_on = false;

    let (get_source, init_playlist) = source_generator(
        rt_handle,
        config.clone(),
        play_control.current_list.clone(),
        play_control.index.clone(),
        playout_stat,
        proc_control.is_terminated.clone(),
    );

    let mut enc_proc = match config.out.mode.as_str() {
        "desktop" => desktop::output(ff_log_format.clone()),
        "stream" => stream::output(ff_log_format.clone()),
        _ => panic!("Output mode doesn't exists!"),
    };

    let mut enc_writer = BufWriter::new(enc_proc.stdin.take().unwrap());

    rt_handle.spawn(stderr_reader(
        enc_proc.stderr.take().unwrap(),
        "Encoder".to_string(),
    ));

    let (ingest_sender, ingest_receiver): (
        SyncSender<(usize, [u8; 65088])>,
        Receiver<(usize, [u8; 65088])>,
    ) = sync_channel(8);

    if config.ingest.enable {
        rt_handle.spawn(ingest_server(
            ff_log_format.clone(),
            ingest_sender,
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

        rt_handle.spawn(stderr_reader(
            dec_proc.stderr.take().unwrap(),
            "Decoder".to_string(),
        ));

        if let Ok(dec_terminator) = dec_proc.terminator() {
            *proc_control.decoder_term.lock().unwrap() = Some(dec_terminator);
        };

        loop {
            if *server_is_running.lock().unwrap() {
                if !live_on {
                    info!("Switch from {} to live ingest", config.processing.mode);

                    if let Err(e) = enc_writer.flush() {
                        error!("Encoder error: {e}")
                    }

                    if let Err(e) = dec_proc.kill() {
                        error!("Decoder error: {e}")
                    };

                    if let Err(e) = dec_proc.wait() {
                        error!("Decoder error: {e}")
                    };

                    live_on = true;

                    *init_playlist.lock().unwrap() = true;
                }

                if let Ok(receive) = ingest_receiver.try_recv() {
                    if let Err(e) = enc_writer.write(&receive.1[..receive.0]) {
                        error!("Ingest receiver error: {:?}", e);

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
                        error!("Reading error from decoder: {:?}", e);

                        break 'source_iter;
                    }
                };

                if dec_bytes_len > 0 {
                    if let Err(e) = enc_writer.write(&buffer[..dec_bytes_len]) {
                        error!("Encoder write error: {:?}", e);

                        break 'source_iter;
                    };
                } else {
                    break;
                }
            }
        }

        if let Err(e) = dec_proc.wait() {
            panic!("Decoder error: {:?}", e)
        };
    }

    sleep(Duration::from_secs(1));

    if let Err(e) = enc_proc.kill() {
        panic!("Encoder error: {:?}", e)
    };

    if let Err(e) = enc_proc.wait() {
        panic!("Encoder error: {:?}", e)
    };
}
