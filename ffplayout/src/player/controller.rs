use std::{
    fmt,
    process::Child,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

#[cfg(not(windows))]
use signal_child::Signalable;

use serde::{Deserialize, Serialize};
// use simplelog::*;

use crate::db::models::Channel;
use crate::utils::errors::ProcessError;

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
    pub channel: Arc<Mutex<Channel>>,
    pub decoder: Arc<Mutex<Option<Child>>>,
    pub encoder: Arc<Mutex<Option<Child>>>,
    pub ingest: Arc<Mutex<Option<Child>>>,
    pub ingest_is_running: Arc<AtomicBool>,
    pub is_terminated: Arc<AtomicBool>,
    pub is_alive: Arc<AtomicBool>,
}

impl ChannelManager {
    pub fn new(channel: Channel) -> Self {
        Self {
            channel: Arc::new(Mutex::new(channel)),
            is_alive: Arc::new(AtomicBool::new(true)),
            ..Default::default()
        }
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

    pub fn update_from(&mut self, other: &Channel, channel_id: i32) {
        self.channels.iter_mut().for_each(|c| {
            let mut channel = c.channel.lock().unwrap();

            if channel.id == channel_id {
                channel.name = other.name.clone();
                channel.preview_url = other.preview_url.clone();
                channel.config_path = other.config_path.clone();
                channel.extra_extensions = other.extra_extensions.clone();
                channel.active = other.active.clone();
                channel.utc_offset = other.utc_offset.clone();
            }
        })
    }

    pub fn stop(mut self, channel_id: i32, unit: ProcessUnit) -> Result<(), ProcessError> {
        for manager in self.channels.iter_mut() {
            let mut channel = manager.channel.lock().unwrap();

            if channel.id == channel_id {
                match unit {
                    Decoder => {
                        if let Some(proc) = manager.decoder.lock().unwrap().as_mut() {
                            #[cfg(not(windows))]
                            proc.term()
                                .map_err(|e| ProcessError::Custom(format!("Decoder: {e}")))?;

                            #[cfg(windows)]
                            proc.kill()
                                .map_err(|e| ProcessError::Custom(format!("Decoder: {e}")))?;
                        }
                    }
                    Encoder => {
                        if let Some(proc) = manager.encoder.lock().unwrap().as_mut() {
                            proc.kill()
                                .map_err(|e| ProcessError::Custom(format!("Encoder: {e}")))?;
                        }
                    }
                    Ingest => {
                        if let Some(proc) = manager.ingest.lock().unwrap().as_mut() {
                            proc.kill()
                                .map_err(|e| ProcessError::Custom(format!("Ingest: {e}")))?;
                        }
                    }
                }

                channel.active = false;
            }
        }

        self.wait(channel_id, unit)?;

        Ok(())
    }

    /// Wait for process to proper close.
    /// This prevents orphaned/zombi processes in system
    pub fn wait(mut self, channel_id: i32, unit: ProcessUnit) -> Result<(), ProcessError> {
        for manager in self.channels.iter_mut() {
            let channel = manager.channel.lock().unwrap();

            if channel.id == channel_id {
                match unit {
                    Decoder => {
                        if let Some(proc) = manager.decoder.lock().unwrap().as_mut() {
                            proc.wait()
                                .map_err(|e| ProcessError::Custom(format!("Decoder: {e}")))?;
                        }
                    }
                    Encoder => {
                        if let Some(proc) = manager.encoder.lock().unwrap().as_mut() {
                            proc.wait()
                                .map_err(|e| ProcessError::Custom(format!("Encoder: {e}")))?;
                        }
                    }
                    Ingest => {
                        if let Some(proc) = manager.ingest.lock().unwrap().as_mut() {
                            proc.wait()
                                .map_err(|e| ProcessError::Custom(format!("Ingest: {e}")))?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn play(controller: &mut ChannelController, channel: Channel) {
    let manager = ChannelManager::new(channel);

    controller.add(manager);
}
