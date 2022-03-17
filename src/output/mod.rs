use notify::{watcher, RecursiveMode, Watcher};
use std::{
    io::{prelude::*, Read},
    path::Path,
    process,
    process::{Command, Stdio},
    sync::{mpsc::channel, Arc, Mutex},
    thread::sleep,
    time::Duration,
};

use simplelog::*;
use tokio::runtime::Handle;

mod desktop;
mod stream;

use crate::utils::{
    ingest_server, sec_to_time, stderr_reader, watch_folder, CurrentProgram, GlobalConfig, Media,
    Source,
};

pub fn play(rt_handle: &Handle) {
    let config = GlobalConfig::global();

    let dec_pid: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

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
            Box::new(CurrentProgram::new(rt_handle.clone())) as Box<dyn Iterator<Item = Media>>
        }
        _ => {
            error!("Process Mode not exists!");
            process::exit(0x0100);
        }
    };

    let dec_settings = config.processing.clone().settings.unwrap();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());

    let mut enc_proc = match config.out.mode.as_str() {
        "desktop" => desktop::output(ff_log_format.clone()),
        "stream" => stream::output(ff_log_format.clone()),
        _ => panic!("Output mode doesn't exists!"),
    };

    rt_handle.spawn(stderr_reader(
        enc_proc.stderr.take().unwrap(),
        "Encoder".to_string(),
    ));

    let mut buffer: [u8; 65424] = [0; 65424];

    ingest_server(ff_log_format.clone());

    for node in get_source {
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

        let mut dec_cmd = vec!["-v", ff_log_format.as_str(), "-hide_banner", "-nostats"];

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

        *dec_pid.lock().unwrap() = dec_proc.id();

        let mut enc_writer = enc_proc.stdin.as_ref().unwrap();
        let dec_reader = dec_proc.stdout.as_mut().unwrap();

        // debug!("Decoder PID: <yellow>{}</>", dec_pid.lock().unwrap());

        rt_handle.spawn(stderr_reader(
            dec_proc.stderr.take().unwrap(),
            "Decoder".to_string(),
        ));

        loop {
            let dec_bytes_len = match dec_reader.read(&mut buffer[..]) {
                Ok(length) => length,
                Err(e) => panic!("Reading error from decoder: {:?}", e),
            };

            if let Err(e) = enc_writer.write(&buffer[..dec_bytes_len]) {
                panic!("Err: {:?}", e)
            };

            if dec_bytes_len == 0 {
                break;
            };
        }

        if let Err(e) = dec_proc.wait() {
            panic!("Decoder error: {:?}", e)
        };
    }

    sleep(Duration::from_secs(1));

    match enc_proc.kill() {
        Ok(_) => info!("Playout done..."),
        Err(e) => panic!("Encoder error: {:?}", e),
    }
}
