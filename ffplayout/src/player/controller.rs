use std::{
    fmt,
    process::Child,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
};

#[cfg(not(windows))]
use signal_child::Signalable;

use log::*;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

use crate::db::models::Channel;
use crate::player::{
    output::{player, write_hls},
    utils::{folder::fill_filler_list, Media},
};
use crate::utils::{
    config::{OutputMode::*, PlayoutConfig},
    errors::ProcessError,
};

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
    pub filter_chain: Option<Arc<Mutex<Vec<String>>>>,
    pub current_date: Arc<Mutex<String>>,
    pub list_init: Arc<AtomicBool>,
    pub current_media: Arc<Mutex<Option<Media>>>,
    pub current_list: Arc<Mutex<Vec<Media>>>,
    pub filler_list: Arc<Mutex<Vec<Media>>>,
    pub current_index: Arc<AtomicUsize>,
    pub filler_index: Arc<AtomicUsize>,
}

impl ChannelManager {
    pub fn new(channel: Channel, config: PlayoutConfig) -> Self {
        Self {
            is_alive: Arc::new(AtomicBool::new(channel.active)),
            channel: Arc::new(Mutex::new(channel)),
            config: Arc::new(Mutex::new(config)),
            current_media: Arc::new(Mutex::new(None)),
            current_list: Arc::new(Mutex::new(vec![Media::new(0, "", false)])),
            filler_list: Arc::new(Mutex::new(vec![])),
            current_index: Arc::new(AtomicUsize::new(0)),
            filler_index: Arc::new(AtomicUsize::new(0)),
            ..Default::default()
        }
    }

    pub fn update_channel(self, other: &Channel) {
        let mut channel = self.channel.lock().unwrap();

        channel.name.clone_from(&other.name);
        channel.preview_url.clone_from(&other.preview_url);
        channel.extra_extensions.clone_from(&other.extra_extensions);
        channel.active.clone_from(&other.active);
        channel.last_date.clone_from(&other.last_date);
        channel.time_shift.clone_from(&other.time_shift);
        channel.utc_offset.clone_from(&other.utc_offset);
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

    pub fn get(&self, id: i32) -> Option<ChannelManager> {
        for manager in self.channels.iter() {
            if manager.channel.lock().unwrap().id == id {
                return Some(manager.clone());
            }
        }

        None
    }

    pub fn remove(&mut self, channel_id: i32) {
        self.channels.retain(|manager| {
            let channel = manager.channel.lock().unwrap();
            channel.id != channel_id
        });
    }

    pub fn run_count(&self) -> usize {
        self.channels
            .iter()
            .filter(|manager| manager.is_alive.load(Ordering::SeqCst))
            .count()
    }
}

pub fn start(db_pool: Pool<Sqlite>, manager: ChannelManager) -> Result<(), ProcessError> {
    let config = manager.config.lock()?.clone();
    let mode = config.output.mode.clone();
    let filler_list = manager.filler_list.clone();

    // Fill filler list, can also be a single file.
    thread::spawn(move || {
        fill_filler_list(&config, Some(filler_list));
    });

    match mode {
        // write files/playlist to HLS m3u8 playlist
        HLS => write_hls(manager, db_pool),
        // play on desktop or stream to a remote target
        _ => player(manager, db_pool),
    }
}
