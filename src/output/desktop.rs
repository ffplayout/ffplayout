use notify::{watcher, RecursiveMode, Watcher};
use std::{
    io::{prelude::*, Read},
    path::Path,
    process,
    process::{Command, Stdio},
    sync::{
        mpsc::channel,
        Arc, Mutex,
    },
    thread::sleep,
    time::Duration,
};

use tokio::runtime::Builder;

use simplelog::*;

use crate::utils::{sec_to_time, watch_folder, Config, CurrentProgram, Media, Source};

pub fn play(config: Config) {
    let dec_pid: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    let get_source = match config.processing.mode.clone().as_str() {
        "folder" => {
            let path = config.storage.path.clone();
            if !Path::new(&path).exists() {
                error!("Folder path not exists: '{path}'");
                process::exit(0x0100);
            }

            info!("Playout in folder mode.");

            let folder_source = Source::new(config.clone());
            let (sender, receiver) = channel();
            let mut watcher = watcher(sender, Duration::from_secs(2)).unwrap();

            watcher
                .watch(path.clone(), RecursiveMode::Recursive)
                .unwrap();

            debug!("Monitor folder: <b><magenta>{}</></b>", path);

            runtime.spawn(watch_folder(
                receiver,
                Arc::clone(&folder_source.nodes),
            ));

            Box::new(folder_source) as Box<dyn Iterator<Item = Media>>
        }
        "playlist" => {
            info!("Playout in playlist mode.");
            Box::new(CurrentProgram::new(config.clone())) as Box<dyn Iterator<Item = Media>>
        }
        _ => {
            error!("Process Mode not exists!");
            process::exit(0x0100);
        }
    };
    // let get_source = CurrentProgram::new(config.clone());
    let dec_settings = config.processing.settings.unwrap();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level);
    let mut enc_cmd = vec![
        "-hide_banner",
        "-nostats",
        "-v",
        ff_log_format.as_str(),
        "-i",
        "pipe:0",
    ];

    let mut enc_filter: Vec<String> = vec![];
    let mut buffer: [u8; 65424] = [0; 65424];

    if config.text.add_text && !config.text.over_pre {
        let text_filter: String = format!(
            "null,zmq=b=tcp\\\\://'{}',drawtext=text='':fontfile='{}'",
            config.text.bind_address.replace(":", "\\:"),
            config.text.fontfile
        );

        enc_filter = vec!["-vf".to_string(), text_filter];
    }

    enc_cmd.append(&mut enc_filter.iter().map(String::as_str).collect());

    debug!("Encoder CMD: <bright-blue>{:?}</>", enc_cmd);

    let mut enc_proc = match Command::new("ffplay")
        .args(enc_cmd)
        .stdin(Stdio::piped())
        // .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            error!("couldn't spawn encoder process: {}", e);
            panic!("couldn't spawn encoder process: {}", e)
        }
        Ok(proc) => proc,
    };

    for node in get_source {
        // println!("Node begin: {:?}", sec_to_time(node.begin.unwrap()));
        let cmd = match node.cmd {
            Some(cmd) => cmd,
            None => break
        };

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
            // .stderr(Stdio::piped())
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

        debug!("Decoder PID: <yellow>{}</>", dec_pid.lock().unwrap());

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
