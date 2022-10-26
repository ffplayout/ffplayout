use std::{
    fs::{self, File},
    path::PathBuf,
    process::exit,
    sync::{Arc, Mutex},
    thread,
};

#[cfg(debug_assertions)]
use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use simplelog::*;

use ffplayout::{
    output::{player, write_hls},
    rpc::json_rpc_server,
    utils::{arg_parse::get_args, get_config},
};

use ffplayout_lib::utils::{
    generate_playlist, import::import_file, init_logging, send_mail, validate_ffmpeg,
    OutputMode::*, PlayerControl, PlayoutStatus, ProcessControl,
};

#[cfg(debug_assertions)]
use ffplayout::utils::Args;

#[cfg(debug_assertions)]
use ffplayout_lib::utils::{mock_time, time_now};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize)]
struct StatusData {
    time_shift: f64,
    date: String,
}

/// Here we create a status file in temp folder.
/// We need this for reading/saving program status.
/// For example when we skip a playing file,
/// we save the time difference, so we stay in sync.
///
/// When file not exists we create it, and when it exists we get its values.
fn status_file(stat_file: &str, playout_stat: &PlayoutStatus) {
    debug!("Start ffplayout v{VERSION}, status file path: <b><magenta>{stat_file}</></b>");

    if !PathBuf::from(stat_file).exists() {
        let data = json!({
            "time_shift": 0.0,
            "date": String::new(),
        });

        let json: String = serde_json::to_string(&data).expect("Serialize status data failed");
        if let Err(e) = fs::write(stat_file, &json) {
            error!("Unable to write to status file <b><magenta>{stat_file}</></b>: {e}");
        };
    } else {
        let stat_file = File::options()
            .read(true)
            .write(false)
            .open(&stat_file)
            .expect("Could not open status file");

        let data: StatusData =
            serde_json::from_reader(stat_file).expect("Could not read status file.");

        *playout_stat.time_shift.lock().unwrap() = data.time_shift;
        *playout_stat.date.lock().unwrap() = data.date;
    }
}

#[cfg(debug_assertions)]
fn fake_time(args: &Args) {
    if let Some(fake_time) = &args.fake_time {
        mock_time::set_mock_time(fake_time);
    } else {
        let local: DateTime<Local> = time_now();
        mock_time::set_mock_time(&local.format("%Y-%m-%dT%H:%M:%S").to_string());
    }
}

fn main() {
    let args = get_args();

    // use fake time function only in debugging mode
    #[cfg(debug_assertions)]
    fake_time(&args);

    let config = get_config(args.clone());
    let config_clone = config.clone();
    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let play_ctl = play_control.clone();
    let play_stat = playout_stat.clone();
    let proc_ctl1 = proc_control.clone();
    let proc_ctl2 = proc_control.clone();
    let messages = Arc::new(Mutex::new(Vec::new()));

    let logging = init_logging(&config, Some(proc_ctl1), Some(messages.clone()));
    CombinedLogger::init(logging).unwrap();

    if let Err(e) = validate_ffmpeg(&config) {
        error!("{e}");
        exit(1);
    };

    if config.general.generate.is_some() {
        // run a simple playlist generator and save them to disk
        if let Err(e) = generate_playlist(&config, None) {
            error!("{e}");
            exit(1);
        };

        exit(0);
    }

    if let Some(path) = args.import {
        if args.date.is_none() {
            error!("Import needs date parameter!");

            exit(1);
        }

        // convert text/m3u file to playlist
        match import_file(&config, &args.date.unwrap(), None, &path) {
            Ok(m) => {
                info!("{m}");
                exit(0);
            }
            Err(e) => {
                error!("{e}");
                exit(1);
            }
        }
    }

    if config.rpc_server.enable {
        // If RPC server is enable we also fire up a JSON RPC server.
        thread::spawn(move || json_rpc_server(config_clone, play_ctl, play_stat, proc_ctl2));
    }

    status_file(&config.general.stat_file, &playout_stat);

    match config.out.mode {
        // write files/playlist to HLS m3u8 playlist
        HLS => write_hls(&config, play_control, playout_stat, proc_control),
        // play on desktop or stream to a remote target
        _ => player(&config, play_control, playout_stat, proc_control),
    }

    info!("Playout done...");

    let msg = messages.lock().unwrap();

    if msg.len() > 0 {
        send_mail(&config, msg.join("\n"));
    }

    drop(msg);
}
