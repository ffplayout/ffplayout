use notify::{watcher, RecursiveMode, Watcher};
use std::{
    io::{prelude::*, BufReader, Read},
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

use process_control::Terminator;
use simplelog::*;
use tokio::runtime::Handle;

mod desktop;
mod stream;

use crate::input::{ingest_server, watch_folder, CurrentProgram, Source};
use crate::utils::{sec_to_time, stderr_reader, GlobalConfig, Media};

pub fn play(rt_handle: &Handle) {
    let config = GlobalConfig::global();
    let dec_settings = config.processing.clone().settings.unwrap();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());

    let server_term: Arc<Mutex<Option<Terminator>>> = Arc::new(Mutex::new(None));
    let is_terminated: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let server_is_running: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let mut init_playlist: Option<Arc<Mutex<bool>>> = None;
    let mut live_on = false;

    let mut buffer: [u8; 65088] = [0; 65088];

    let get_source = match config.processing.clone().mode.as_str() {
        "folder" => {
            let path = config.storage.path.clone();
            if !Path::new(&path).exists() {
                error!("Folder path not exists: '{path}'");
                process::exit(0x0100);
            }

            info!("Playout in folder mode.");

            let folder_source = Source::new();
            let (sender, receiver) = channel();
            let mut watcher = watcher(sender, Duration::from_secs(2)).unwrap();

            watcher
                .watch(path.clone(), RecursiveMode::Recursive)
                .unwrap();

            debug!("Monitor folder: <b><magenta>{}</></b>", path);

            rt_handle.spawn(watch_folder(receiver, Arc::clone(&folder_source.nodes)));

            Box::new(folder_source) as Box<dyn Iterator<Item = Media>>
        }
        "playlist" => {
            info!("Playout in playlist mode");
            let program = CurrentProgram::new(rt_handle.clone(), is_terminated.clone());
            init_playlist = Some(program.init.clone());
            Box::new(program) as Box<dyn Iterator<Item = Media>>
        }
        _ => {
            error!("Process Mode not exists!");
            process::exit(0x0100);
        }
    };

    let mut enc_proc = match config.out.mode.as_str() {
        "desktop" => desktop::output(ff_log_format.clone()),
        "stream" => stream::output(ff_log_format.clone()),
        _ => panic!("Output mode doesn't exists!"),
    };

    rt_handle.spawn(stderr_reader(
        enc_proc.stderr.take().unwrap(),
        "Encoder".to_string(),
    ));

    let (ingest_sender, ingest_receiver): (
        SyncSender<(usize, [u8; 65088])>,
        Receiver<(usize, [u8; 65088])>,
    ) = sync_channel(4);

    if config.ingest.enable {
        rt_handle.spawn(ingest_server(
            ff_log_format.clone(),
            ingest_sender,
            rt_handle.clone(),
            server_term.clone(),
            is_terminated.clone(),
            server_is_running.clone(),
        ));
    }

    'source_iter: for node in get_source {
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
        debug!("Decoder CMD: <bright-blue>{:?}</>", dec_cmd);

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

        let mut enc_writer = enc_proc.stdin.as_ref().unwrap();
        let mut dec_reader = BufReader::new(dec_proc.stdout.take().unwrap());

        rt_handle.spawn(stderr_reader(
            dec_proc.stderr.take().unwrap(),
            "Decoder".to_string(),
        ));

        let mut kill_dec = true;

        loop {
            if *server_is_running.lock().unwrap() {
                if let Ok(receive) = ingest_receiver.try_recv() {
                    if let Err(e) = enc_writer.write(&receive.1[..receive.0]) {
                        error!("Ingest receiver error: {:?}", e);

                        break 'source_iter;
                    };
                }

                live_on = true;

                if kill_dec {
                    info!("Switch from {} to live ingest", config.processing.mode);

                    if let Err(e) = dec_proc.kill() {
                        error!("Decoder error: {e}")
                    };

                    if let Err(e) = dec_proc.wait() {
                        error!("Decoder error: {e}")
                    };

                    kill_dec = false;

                    if let Some(init) = &init_playlist {
                        *init.lock().unwrap() = true;
                    }
                }
            } else {
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
                    if live_on {
                        info!("Switch from live ingest to {}", config.processing.mode);

                        live_on = false;
                    }

                    enc_writer.flush().unwrap();

                    break;
                }
            }
        }

        if let Err(e) = dec_proc.wait() {
            panic!("Decoder error: {:?}", e)
        };
    }

    *is_terminated.lock().unwrap() = true;

    if let Some(server) = &*server_term.lock().unwrap() {
        unsafe {
            if let Ok(_) = server.terminate() {
                info!("Terminate ingest server done");
            }
        }
    };

    sleep(Duration::from_secs(1));

    match enc_proc.kill() {
        Ok(_) => info!("Playout done..."),
        Err(e) => panic!("Encoder error: {:?}", e),
    }

    if let Err(e) = enc_proc.wait() {
        error!("Encoder: {e}")
    };
}
