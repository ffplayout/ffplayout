use std::{
    fs::{self, File},
    path::PathBuf,
    process::exit,
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread,
};

#[cfg(debug_assertions)]
use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use simplelog::*;

use ffplayout::{
    output::{player, write_hls},
    rpc::run_server,
    utils::{arg_parse::get_args, get_config},
};

use ffplayout_lib::utils::{
    errors::ProcError, folder::fill_filler_list, generate_playlist, get_date, import::import_file,
    init_logging, is_remote, send_mail, test_tcp_port, validate_ffmpeg, validate_playlist,
    JsonPlaylist, OutputMode::*, PlayerControl, PlayoutStatus, ProcessControl,
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
fn status_file(stat_file: &str, playout_stat: &PlayoutStatus) -> Result<(), ProcError> {
    debug!("Start ffplayout v{VERSION}, status file path: <b><magenta>{stat_file}</></b>");

    if !PathBuf::from(stat_file).exists() {
        let data = json!({
            "time_shift": 0.0,
            "date": String::new(),
        });

        let json: String = serde_json::to_string(&data)?;
        if let Err(e) = fs::write(stat_file, json) {
            error!("Unable to write to status file <b><magenta>{stat_file}</></b>: {e}");
        };
    } else {
        let stat_file = File::options().read(true).write(false).open(stat_file)?;
        let data: StatusData = serde_json::from_reader(stat_file)?;

        *playout_stat.time_shift.lock().unwrap() = data.time_shift;
        *playout_stat.date.lock().unwrap() = data.date;
    }

    Ok(())
}

/// Set fake time for debugging.
/// When no time is given, we use the current time.
/// When a time is given, we use this time instead.
#[cfg(debug_assertions)]
fn fake_time(args: &Args) {
    if let Some(fake_time) = &args.fake_time {
        mock_time::set_mock_time(fake_time);
    } else {
        let local: DateTime<Local> = time_now();
        mock_time::set_mock_time(&local.format("%Y-%m-%dT%H:%M:%S").to_string());
    }
}

/// Main function.
/// Here we check the command line arguments and start the player.
/// We also start a JSON RPC server if enabled.
fn main() -> Result<(), ProcError> {
    let args = get_args();

    // use fake time function only in debugging mode
    #[cfg(debug_assertions)]
    fake_time(&args);

    let mut config = get_config(args.clone())?;
    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let play_ctl1 = play_control.clone();
    let play_ctl2 = play_control.clone();
    let play_stat = playout_stat.clone();
    let proc_ctl1 = proc_control.clone();
    let proc_ctl2 = proc_control.clone();
    let messages = Arc::new(Mutex::new(Vec::new()));

    // try to create logging folder, if not exist
    if config.logging.log_to_file && config.logging.path.is_dir() {
        if let Err(e) = fs::create_dir_all(&config.logging.path) {
            println!("Logging path not exists! {e}");

            exit(1);
        }
    }

    let logging = init_logging(&config, Some(proc_ctl1), Some(messages.clone()));
    CombinedLogger::init(logging)?;

    if let Err(e) = validate_ffmpeg(&mut config) {
        error!("{e}");
        exit(1);
    };

    let config_clone1 = config.clone();
    let config_clone2 = config.clone();

    if !matches!(config.processing.audio_channels, 2 | 4 | 6 | 8) {
        error!(
            "Encoding {} channel(s) is not allowed. Only 2, 4, 6 and 8 channels are supported!",
            config.processing.audio_channels
        );
        exit(1);
    }

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

    if args.validate {
        let play_ctl3 = play_control.clone();
        let mut playlist_path = config.playlist.path.clone();
        let start_sec = config.playlist.start_sec.unwrap();
        let date = get_date(false, start_sec, false);

        if playlist_path.is_dir() || is_remote(&playlist_path.to_string_lossy()) {
            let d: Vec<&str> = date.split('-').collect();
            playlist_path = playlist_path
                .join(d[0])
                .join(d[1])
                .join(date.clone())
                .with_extension("json");
        }

        let f = File::options()
            .read(true)
            .write(false)
            .open(&playlist_path)?;

        let playlist: JsonPlaylist = serde_json::from_reader(f)?;

        validate_playlist(
            config,
            play_ctl3,
            playlist,
            Arc::new(AtomicBool::new(false)),
        );

        exit(0);
    }

    if config.rpc_server.enable {
        // If RPC server is enable we also fire up a JSON RPC server.

        if !test_tcp_port(&config.rpc_server.address) {
            exit(1)
        }

        thread::spawn(move || run_server(config_clone1, play_ctl1, play_stat, proc_ctl2));
    }

    status_file(&config.general.stat_file, &playout_stat)?;

    debug!(
        "Use config: <b><magenta>{}</></b>",
        config.general.config_path
    );

    // Fill filler list, can also be a single file.
    thread::spawn(move || {
        fill_filler_list(&config_clone2, Some(play_ctl2));
    });

    match config.out.mode {
        // write files/playlist to HLS m3u8 playlist
        HLS => write_hls(&config, play_control, playout_stat, proc_control),
        // play on desktop or stream to a remote target
        _ => player(&config, &play_control, playout_stat, proc_control),
    }

    info!("Playout done...");

    let msg = messages.lock().unwrap();

    if msg.len() > 0 {
        send_mail(&config, msg.join("\n"));
    }

    drop(msg);

    Ok(())
}
