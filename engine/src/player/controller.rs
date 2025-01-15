use std::{
    fmt,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use async_walkdir::{Filtering, WalkDir};
use log::*;
use m3u8_rs::Playlist;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio::{
    fs,
    io::{self, AsyncReadExt},
    process::{Child, ChildStdout},
    sync::Mutex,
};
use tokio_stream::StreamExt;

use crate::player::{
    output::{player, write_hls},
    utils::{folder::fill_filler_list, Media},
};
use crate::utils::{
    config::{OutputMode::*, PlayoutConfig},
    errors::{ProcessError, ServiceError},
};
use crate::ARGS;
use crate::{
    db::{handles, models::Channel},
    utils::logging::Target,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
            Self::Decoder => write!(f, "Decoder"),
            Self::Encoder => write!(f, "Encoder"),
            Self::Ingest => write!(f, "Ingest"),
        }
    }
}

use ProcessUnit::*;

#[derive(Clone, Debug, Default)]
pub struct ChannelManager {
    pub db_pool: Option<Pool<Sqlite>>,
    pub config: Arc<Mutex<PlayoutConfig>>,
    pub channel: Arc<Mutex<Channel>>,
    pub decoder: Arc<Mutex<Option<Child>>>,
    pub encoder: Arc<Mutex<Option<Child>>>,
    pub ingest: Arc<Mutex<Option<Child>>>,
    pub ingest_stdout: Arc<Mutex<Option<ChildStdout>>>,
    pub ingest_is_running: Arc<AtomicBool>,
    pub is_terminated: Arc<AtomicBool>,
    pub is_alive: Arc<AtomicBool>,
    pub is_processing: Arc<AtomicBool>,
    pub filter_chain: Option<Arc<Mutex<Vec<String>>>>,
    pub current_date: Arc<Mutex<String>>,
    pub list_init: Arc<AtomicBool>,
    pub current_media: Arc<Mutex<Option<Media>>>,
    pub current_list: Arc<Mutex<Vec<Media>>>,
    pub filler_list: Arc<Mutex<Vec<Media>>>,
    pub current_index: Arc<AtomicUsize>,
    pub filler_index: Arc<AtomicUsize>,
    pub run_count: Arc<AtomicUsize>,
}

impl ChannelManager {
    pub fn new(db_pool: Option<Pool<Sqlite>>, channel: Channel, config: PlayoutConfig) -> Self {
        Self {
            db_pool,
            is_alive: Arc::new(AtomicBool::new(false)),
            channel: Arc::new(Mutex::new(channel)),
            config: Arc::new(Mutex::new(config)),
            list_init: Arc::new(AtomicBool::new(true)),
            current_media: Arc::new(Mutex::new(None)),
            current_list: Arc::new(Mutex::new(vec![Media::default()])),
            filler_list: Arc::new(Mutex::new(vec![])),
            current_index: Arc::new(AtomicUsize::new(0)),
            filler_index: Arc::new(AtomicUsize::new(0)),
            run_count: Arc::new(AtomicUsize::new(0)),
            ..Default::default()
        }
    }

    pub async fn update_channel(self, other: &Channel) {
        let mut channel = self.channel.lock().await;

        channel.name.clone_from(&other.name);
        channel.preview_url.clone_from(&other.preview_url);
        channel.extra_extensions.clone_from(&other.extra_extensions);
        channel.active.clone_from(&other.active);
        channel.last_date.clone_from(&other.last_date);
        channel.time_shift.clone_from(&other.time_shift);
        channel.timezone.clone_from(&other.timezone);
    }

    pub async fn update_config(&self, new_config: PlayoutConfig) {
        let mut config = self.config.lock().await;
        *config = new_config;
    }

    pub async fn start(&self) {
        if !self.is_alive.load(Ordering::SeqCst) {
            self.run_count.fetch_add(1, Ordering::SeqCst);
            self.is_alive.store(true, Ordering::SeqCst);
            self.is_terminated.store(false, Ordering::SeqCst);
            self.list_init.store(true, Ordering::SeqCst);

            let pool_clone = self.db_pool.clone().unwrap();
            let self_clone = self.clone();
            let channel_id = self.channel.lock().await.id;

            if let Err(e) = handles::update_player(&pool_clone, channel_id, true).await {
                error!(target: Target::all(), channel = channel_id; "Unable write to player status: {e}");
            };

            tokio::spawn({
                let self_clone = self_clone.clone();
                async move {
                    let mut run_endless = true;

                    while run_endless {
                        let run_count = self_clone.run_count.clone();
                        let self_clone2 = self_clone.clone();

                        if let Err(e) = start_channel(self_clone2).await {
                            run_count.fetch_sub(1, Ordering::SeqCst);
                            error!("{e}");
                        };

                        if self_clone.channel.lock().await.active {
                            self_clone.run_count.fetch_add(1, Ordering::SeqCst);
                            self_clone.is_alive.store(true, Ordering::SeqCst);
                            self_clone.is_terminated.store(false, Ordering::SeqCst);
                            self_clone.list_init.store(true, Ordering::SeqCst);

                            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                        } else {
                            run_endless = false;
                        }
                    }

                    trace!("Async start done");
                }
            });
        }
    }

    pub async fn foreground_start(&self, index: usize) {
        if !self.is_alive.load(Ordering::SeqCst) {
            self.run_count.fetch_add(1, Ordering::SeqCst);
            self.is_alive.store(true, Ordering::SeqCst);
            self.is_terminated.store(false, Ordering::SeqCst);
            self.list_init.store(true, Ordering::SeqCst);

            let pool_clone = self.db_pool.clone().unwrap();
            let self_clone = self.clone();
            let channel_id = self.channel.lock().await.id;

            if let Err(e) = handles::update_player(&pool_clone, channel_id, true).await {
                error!(target: Target::all(), channel = channel_id; "Unable write to player status: {e}");
            };

            if index + 1 == ARGS.channel.clone().unwrap_or_default().len() {
                let run_count = self_clone.run_count.clone();

                if let Err(e) = start_channel(self_clone).await {
                    run_count.fetch_sub(1, Ordering::SeqCst);
                    error!("{e}");
                }
            } else {
                tokio::spawn(async move {
                    let run_count = self_clone.run_count.clone();

                    if let Err(e) = start_channel(self_clone).await {
                        run_count.fetch_sub(1, Ordering::SeqCst);
                        error!("{e}");
                    };
                });
            }
        }
    }

    pub async fn stop(&self, unit: ProcessUnit) -> Result<(), ProcessError> {
        match unit {
            Decoder => {
                if let Some(proc) = self.decoder.lock().await.as_mut() {
                    proc.kill()
                        .await
                        .map_err(|e| ProcessError::Custom(format!("Decoder: {e}")))?;
                }
            }
            Encoder => {
                if let Some(proc) = self.encoder.lock().await.as_mut() {
                    proc.kill()
                        .await
                        .map_err(|e| ProcessError::Custom(format!("Encoder: {e}")))?;
                }
            }
            Ingest => {
                if let Some(proc) = self.ingest.lock().await.as_mut() {
                    proc.kill()
                        .await
                        .map_err(|e| ProcessError::Custom(format!("Ingest: {e}")))?;
                }
            }
        }

        self.wait(unit).await?;

        Ok(())
    }

    /// Wait for process to proper close.
    /// This prevents orphaned/zombi processes in system
    pub async fn wait(&self, unit: ProcessUnit) -> Result<(), ProcessError> {
        let child = match unit {
            Decoder => &self.decoder,
            Encoder => &self.encoder,
            Ingest => &self.ingest,
        };

        if let Some(proc) = child.lock().await.as_mut() {
            let mut counter = 0;
            loop {
                match proc.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if counter > 300 {
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                        counter += 1;
                    }
                    Err(e) => return Err(ProcessError::Custom(format!("{unit}: {e}"))),
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        Ok(())
    }

    /// No matter what is running, terminate them all.
    pub async fn stop_all(&self, permanent: bool) -> Result<(), ServiceError> {
        let channel_id = self.channel.lock().await.id;

        if permanent {
            let pool = self.db_pool.clone().unwrap();

            if self.is_alive.load(Ordering::SeqCst) {
                debug!(target: Target::all(), channel = channel_id; "Deactivate playout and stop all child processes from channel: <yellow>{channel_id}</>");
            }

            if let Err(e) = handles::update_player(&pool, channel_id, false).await {
                error!(target: Target::all(), channel = channel_id; "Player status cannot be written: {e}");
            };
        } else {
            debug!(target: Target::all(), channel = channel_id; "Stop all child processes from channel: <yellow>{channel_id}</>");
        }

        self.is_terminated.store(true, Ordering::SeqCst);
        self.is_alive.store(false, Ordering::SeqCst);
        self.ingest_is_running.store(false, Ordering::SeqCst);
        self.run_count.fetch_sub(1, Ordering::SeqCst);

        for unit in [Decoder, Encoder, Ingest] {
            if let Err(e) = self.stop(unit).await {
                if !e.to_string().contains("exited process") {
                    error!(target: Target::all(), channel = channel_id; "{e}");
                }
            }
        }

        Ok(())
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

    pub async fn get(&self, id: i32) -> Option<ChannelManager> {
        for manager in &self.channels {
            if manager.channel.lock().await.id == id {
                return Some(manager.clone());
            }
        }

        None
    }

    pub async fn remove(&mut self, channel_id: i32) {
        let mut indices = Vec::new();

        for (i, manager) in self.channels.iter().enumerate() {
            let channel = manager.channel.lock().await;
            if channel.id == channel_id {
                indices.push(i);
            }
        }

        indices.reverse();

        for i in indices {
            self.channels.remove(i);
        }
    }

    pub fn run_count(&self) -> usize {
        self.channels
            .iter()
            .filter(|manager| manager.is_alive.load(Ordering::SeqCst))
            .count()
    }
}

async fn start_channel(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = manager.config.lock().await.clone();
    let mode = config.output.mode.clone();
    let filler_list = manager.filler_list.clone();
    let channel_id = config.general.channel_id;

    drain_hls_path(&config.channel.public).await?;

    debug!(target: Target::all(), channel = channel_id; "Start ffplayout v{VERSION}, channel: <yellow>{channel_id}</>");

    // Fill filler list, can also be a single file.
    // INFO: Was running in a thread, but when it runs in a tokio task and
    // after start a filler is needed, the first one will be ignored because the list is not filled.

    if filler_list.lock().await.is_empty() {
        fill_filler_list(&config, Some(filler_list)).await;
    }

    match mode {
        // write files/playlist to HLS m3u8 playlist
        HLS => write_hls(manager).await,
        // play on desktop or stream to a remote target
        _ => player(manager).await,
    }
}

pub async fn drain_hls_path(path: &Path) -> io::Result<()> {
    let m3u8_files = find_m3u8_files(path).await?;
    let mut pl_segments = vec![];

    for file in m3u8_files {
        let mut file = fs::File::open(file).await?;
        let mut bytes: Vec<u8> = Vec::new();
        file.read_to_end(&mut bytes).await?;

        if let Ok(Playlist::MediaPlaylist(pl)) = m3u8_rs::parse_playlist_res(&bytes) {
            for segment in pl.segments {
                pl_segments.push(segment.uri);
            }
        };
    }

    delete_old_segments(path, &pl_segments).await
}

/// Recursively searches for all files with the .m3u8 extension in the specified path.
async fn find_m3u8_files(path: &Path) -> io::Result<Vec<String>> {
    let mut m3u8_files = Vec::new();

    let mut entries = WalkDir::new(path).filter(move |entry| async move {
        if entry.path().is_file() && entry.path().extension().is_some_and(|ext| ext == "m3u8") {
            return Filtering::Continue;
        }

        Filtering::Ignore
    });

    loop {
        match entries.next().await {
            Some(Ok(entry)) => {
                m3u8_files.push(entry.path().to_string_lossy().to_string());
            }
            Some(Err(e)) => {
                error!(target: Target::file_mail(), "error: {e}");
                break;
            }
            None => break,
        }
    }

    Ok(m3u8_files)
}

/// Check if segment is in playlist, if not, delete it.
async fn delete_old_segments<P: AsRef<Path> + Clone + std::fmt::Debug>(
    path: P,
    pl_segments: &[String],
) -> io::Result<()> {
    let mut entries = WalkDir::new(path).filter(move |entry| async move {
        if entry.path().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "ts" || ext == "vtt")
        {
            return Filtering::Continue;
        }

        Filtering::Ignore
    });

    loop {
        match entries.next().await {
            Some(Ok(entry)) => {
                let filename = entry.file_name().to_string_lossy().to_string();

                if !pl_segments.contains(&filename) {
                    fs::remove_file(entry.path()).await?;
                }
            }
            Some(Err(e)) => {
                error!(target: Target::file_mail(), "error: {e}");
                break;
            }
            None => break,
        }
    }

    Ok(())
}
