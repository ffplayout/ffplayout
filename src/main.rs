extern crate log;
extern crate simplelog;

use std::{
    fs::{self, File},
    path::PathBuf,
    process::exit,
    sync::{Arc, Mutex},
    thread,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use simplelog::*;

mod filter;
mod input;
mod macros;
mod output;
mod rpc;
#[cfg(test)]
mod tests;
mod utils;

use crate::output::{player, write_hls};
use crate::utils::{
    generate_playlist, init_logging, send_mail, validate_ffmpeg, GlobalConfig, PlayerControl,
    PlayoutStatus, ProcessControl,
};
use rpc::json_rpc_server;

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
    if !PathBuf::from(stat_file).exists() {
        let data = json!({
            "time_shift": 0.0,
            "date": String::new(),
        });

        let json: String = serde_json::to_string(&data).expect("Serialize status data failed");
        fs::write(stat_file, &json).expect("Unable to write file");
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

fn main() {
    let config = GlobalConfig::new();
    let config_clone = config.clone();
    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();
    let messages = Arc::new(Mutex::new(Vec::new()));

    let logging = init_logging(&config, messages.clone());
    CombinedLogger::init(logging).unwrap();

    validate_ffmpeg(&config);
    status_file(&config.general.stat_file, &playout_stat);

    if let Some(range) = config.general.generate.clone() {
        // run a simple playlist generator and save them to disk
        generate_playlist(&config, range);

        exit(0);
    }

    let play_ctl = play_control.clone();
    let play_stat = playout_stat.clone();
    let proc_ctl = proc_control.clone();

    if config.rpc_server.enable {
        // If RPC server is enable we also fire up a JSON RPC server.
        thread::spawn(move || json_rpc_server(config_clone, play_ctl, play_stat, proc_ctl));
    }

    if &config.out.mode.to_lowercase() == "hls" {
        // write files/playlist to HLS m3u8 playlist
        write_hls(&config, play_control, playout_stat, proc_control);
    } else {
        // play on desktop or stream to a remote target
        player(&config, play_control, playout_stat, proc_control);
    }

    let msg = messages.lock().unwrap();

    if msg.len() > 0 {
        send_mail(&config, msg.join("\n"));
    }

    info!("Playout done...");
}
