extern crate log;
extern crate simplelog;

use std::{
    {fs, fs::File},
    path::PathBuf,
    process::exit,
    thread,

};

use serde::{Deserialize, Serialize};
use serde_json::json;
use simplelog::*;

mod filter;
mod input;
mod output;
mod rpc;
mod utils;

use crate::output::{player, write_hls};
use crate::utils::{
    generate_playlist, init_config, init_logging, validate_ffmpeg, GlobalConfig, PlayerControl,
    PlayoutStatus, ProcessControl,
};
use rpc::json_rpc_server;

#[derive(Serialize, Deserialize)]
struct StatusData {
    time_shift: f64,
    date: String,
}

fn main() {
    init_config();
    let config = GlobalConfig::global();
    let play_control = PlayerControl::new();
    let playout_stat = PlayoutStatus::new();
    let proc_control = ProcessControl::new();

    if !PathBuf::from(config.general.stat_file.clone()).exists() {
        let data = json!({
            "time_shift": 0.0,
            "date": String::new(),
        });

        let json: String = serde_json::to_string(&data).expect("Serialize status data failed");
        fs::write(config.general.stat_file.clone(), &json).expect("Unable to write file");
    } else {
        let stat_file = File::options()
            .read(true)
            .write(false)
            .open(&config.general.stat_file)
            .expect("Could not open status file");

        let data: StatusData =
            serde_json::from_reader(stat_file).expect("Could not read status file.");

        *playout_stat.time_shift.lock().unwrap() = data.time_shift;
        *playout_stat.date.lock().unwrap() = data.date;
    }

    let logging = init_logging();
    CombinedLogger::init(logging).unwrap();

    validate_ffmpeg();

    if let Some(range) = config.general.generate.clone() {
        generate_playlist(range);

        exit(0);
    }

    let play_ctl = play_control.clone();
    let play_stat = playout_stat.clone();
    let proc_ctl = proc_control.clone();

    if config.rpc_server.enable {
        thread::spawn( move || json_rpc_server(
            play_ctl,
            play_stat,
            proc_ctl,
        ));
    }

    if &config.out.mode.to_lowercase() == "hls" {
        write_hls(play_control, playout_stat, proc_control);
    } else {
        player(play_control, playout_stat, proc_control);
    }

    info!("Playout done...");
}
