use std::process::Command;

use log::*;

use crate::player::utils::get_data_map;

use crate::player::controller::ChannelManager;

pub fn run(manager: ChannelManager) {
    let task_path = manager.config.lock().unwrap().task.path.clone();

    let obj = serde_json::to_string(&get_data_map(&manager)).unwrap();
    trace!("Run task: {obj}");

    match Command::new(task_path).arg(obj).spawn() {
        Ok(mut c) => {
            let status = c.wait().expect("Error in waiting for the task process!");

            if !status.success() {
                error!("Process stops with error.");
            }
        }
        Err(e) => {
            error!("Couldn't spawn task runner: {e}");
        }
    }
}
