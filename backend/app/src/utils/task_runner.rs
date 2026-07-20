use log::*;
use tokio::process::Command;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

use crate::player::{controller::ChannelManager, utils::get_data_map};

const TASK_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run(manager: ChannelManager, cancel: CancellationToken) {
    let channel_id = manager.id;
    let task_path = manager.config.read().await.task.path.clone();

    let obj = match serde_json::to_string(&get_data_map(&manager).await) {
        Ok(obj) => obj,
        Err(error) => {
            error!(channel = channel_id; "Could not serialize task data: {error}");
            return;
        }
    };
    trace!("Run task: {obj}");

    match Command::new(task_path).arg(obj).kill_on_drop(true).spawn() {
        Ok(mut child) => {
            tokio::select! {
                status = child.wait() => match status {
                    Ok(status) if status.success() => {}
                    Ok(status) => error!(channel = channel_id; "Task process stopped with status {status}"),
                    Err(error) => error!(channel = channel_id; "Could not wait for task process: {error}"),
                },
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
                _ = sleep(TASK_TIMEOUT) => {
                    warn!(channel = channel_id; "Task process exceeded {} seconds and was terminated", TASK_TIMEOUT.as_secs());
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            }
        }
        Err(error) => {
            error!(channel = channel_id; "Could not spawn task process: {error}");
        }
    }
}
