use std::process::Command;

use simplelog::*;

use crate::utils::get_data_map;
use ffplayout_lib::utils::{config::PlayoutConfig, Media, PlayoutStatus};

pub fn run(config: PlayoutConfig, node: Media, playout_stat: PlayoutStatus, server_running: bool) {
    let obj =
        serde_json::to_string(&get_data_map(&config, node, &playout_stat, server_running)).unwrap();
    trace!("Run task: {obj}");

    match Command::new(config.task.path).arg(obj).spawn() {
        Ok(mut c) => {
            let status = c.wait().expect("Error in waiting for the task process!");

            if !status.success() {
                error!("Process stops with error.");
            }
        }
        Err(e) => {
            error!("Couldn't spawn task runner: {e}")
        }
    }
}
