use std::{
    fmt,
    process::Child,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[cfg(not(windows))]
use signal_child::Signalable;

use log::*;
use serde::{Deserialize, Serialize};

use crate::db::models::Channel;
use crate::player::output::{player, write_hls};
use crate::utils::{config::PlayoutConfig, errors::ProcessError};

/// Defined process units.
#[derive(Clone, Debug, Default, Copy, Eq, Serialize, Deserialize, PartialEq)]
pub enum ProcessUnit {
    #[default]
    Decoder,
    Encoder,
    Ingest,
}

impl fmt::Display for ProcessUnit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ProcessUnit::Decoder => write!(f, "Decoder"),
            ProcessUnit::Encoder => write!(f, "Encoder"),
            ProcessUnit::Ingest => write!(f, "Ingest"),
        }
    }
}

use ProcessUnit::*;

#[derive(Clone, Debug, Default)]
pub struct ChannelManager {
    pub config: Arc<Mutex<PlayoutConfig>>,
    pub channel: Arc<Mutex<Channel>>,
    pub decoder: Arc<Mutex<Option<Child>>>,
    pub encoder: Arc<Mutex<Option<Child>>>,
    pub ingest: Arc<Mutex<Option<Child>>>,
    pub ingest_is_running: Arc<AtomicBool>,
    pub is_terminated: Arc<AtomicBool>,
    pub is_alive: Arc<AtomicBool>,
}

impl ChannelManager {
    pub fn new(channel: Channel, config: PlayoutConfig) -> Self {
        Self {
            is_alive: Arc::new(AtomicBool::new(channel.active)),
            channel: Arc::new(Mutex::new(channel)),
            config: Arc::new(Mutex::new(config)),
            ..Default::default()
        }
    }

    pub fn update_channel(self, other: &Channel) {
        let mut channel = self.channel.lock().unwrap();

        channel.name = other.name.clone();
        channel.preview_url = other.preview_url.clone();
        channel.config_path = other.config_path.clone();
        channel.extra_extensions = other.extra_extensions.clone();
        channel.active = other.active.clone();
        channel.modified = other.modified.clone();
        channel.time_shift = other.time_shift.clone();
        channel.utc_offset = other.utc_offset.clone();
    }

    pub fn stop(&self, unit: ProcessUnit) -> Result<(), ProcessError> {
        let mut channel = self.channel.lock()?;

        match unit {
            Decoder => {
                if let Some(proc) = self.decoder.lock()?.as_mut() {
                    #[cfg(not(windows))]
                    proc.term()
                        .map_err(|e| ProcessError::Custom(format!("Decoder: {e}")))?;

                    #[cfg(windows)]
                    proc.kill()
                        .map_err(|e| ProcessError::Custom(format!("Decoder: {e}")))?;
                }
            }
            Encoder => {
                if let Some(proc) = self.encoder.lock()?.as_mut() {
                    proc.kill()
                        .map_err(|e| ProcessError::Custom(format!("Encoder: {e}")))?;
                }
            }
            Ingest => {
                if let Some(proc) = self.ingest.lock()?.as_mut() {
                    proc.kill()
                        .map_err(|e| ProcessError::Custom(format!("Ingest: {e}")))?;
                }
            }
        }

        channel.active = false;

        self.wait(unit)?;

        Ok(())
    }

    /// Wait for process to proper close.
    /// This prevents orphaned/zombi processes in system
    pub fn wait(&self, unit: ProcessUnit) -> Result<(), ProcessError> {
        match unit {
            Decoder => {
                if let Some(proc) = self.decoder.lock().unwrap().as_mut() {
                    proc.wait()
                        .map_err(|e| ProcessError::Custom(format!("Decoder: {e}")))?;
                }
            }
            Encoder => {
                if let Some(proc) = self.encoder.lock().unwrap().as_mut() {
                    proc.wait()
                        .map_err(|e| ProcessError::Custom(format!("Encoder: {e}")))?;
                }
            }
            Ingest => {
                if let Some(proc) = self.ingest.lock().unwrap().as_mut() {
                    proc.wait()
                        .map_err(|e| ProcessError::Custom(format!("Ingest: {e}")))?;
                }
            }
        }

        Ok(())
    }

    /// No matter what is running, terminate them all.
    pub fn stop_all(&self) {
        debug!("Stop all child processes");
        self.is_terminated.store(true, Ordering::SeqCst);
        self.ingest_is_running.store(false, Ordering::SeqCst);

        if self.is_alive.load(Ordering::SeqCst) {
            self.is_alive.store(false, Ordering::SeqCst);

            trace!("Playout is alive and processes are terminated");

            for unit in [Decoder, Encoder, Ingest] {
                if let Err(e) = self.stop(unit) {
                    if !e.to_string().contains("exited process") {
                        error!("{e}")
                    }
                }
                if let Err(e) = self.wait(unit) {
                    if !e.to_string().contains("exited process") {
                        error!("{e}")
                    }
                }
            }
        }
    }
}

/// Global playout control, for move forward/backward clip, or resetting playlist/state.
#[derive(Clone, Debug)]
pub struct PlayoutStatus {
    pub chain: Option<Arc<Mutex<Vec<String>>>>,
    pub current_date: Arc<Mutex<String>>,
    pub date: Arc<Mutex<String>>,
    pub list_init: Arc<AtomicBool>,
    pub time_shift: Arc<Mutex<f64>>,
}

impl PlayoutStatus {
    pub fn new() -> Self {
        Self {
            chain: None,
            current_date: Arc::new(Mutex::new(String::new())),
            date: Arc::new(Mutex::new(String::new())),
            list_init: Arc::new(AtomicBool::new(true)),
            time_shift: Arc::new(Mutex::new(0.0)),
        }
    }
}

impl Default for PlayoutStatus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChannelController {
    pub channels: Vec<ChannelManager>,
}

impl ChannelController {
    pub fn new() -> Self {
        Self { channels: vec![] }
    }

    pub fn add(&mut self, manager: ChannelManager) {
        self.channels.push(manager);
    }

    pub fn remove(&mut self, channel_id: i32) {
        self.channels.retain(|manager| {
            let channel = manager.channel.lock().unwrap();
            channel.id != channel_id
        });
    }
}

pub fn start(controller: Arc<Mutex<ChannelManager>>) -> Result<(), ProcessError> {
    let config = controller.lock()?.config.lock()?.clone();

    match config.out.mode {
        // write files/playlist to HLS m3u8 playlist
        HLS => write_hls(&config, play_control, playout_stat, proc_control),
        // play on desktop or stream to a remote target
        _ => player(&config, &play_control, playout_stat, proc_control),
    };

    Ok(())
}
