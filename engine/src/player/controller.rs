use std::{
    cmp, fmt,
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
    time::{sleep, Duration, Instant},
};
use tokio_stream::StreamExt;

use crate::player::{
    output::player,
    utils::{folder::fill_filler_list, Media},
};
use crate::utils::{config::PlayoutConfig, errors::ServiceError};
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

#[derive(Clone, Debug)]
pub struct ChannelManager {
    pub db_pool: Pool<Sqlite>,
    pub config: Arc<Mutex<PlayoutConfig>>,
    pub channel: Arc<Mutex<Channel>>,
    pub decoder: Arc<Mutex<Option<Child>>>,
    pub encoder: Arc<Mutex<Option<Child>>>,
    pub ingest: Arc<Mutex<Option<Child>>>,
    pub ingest_stdout: Arc<Mutex<Option<ChildStdout>>>,
    pub ingest_is_alive: Arc<AtomicBool>,
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
}

impl ChannelManager {
    pub fn new(db_pool: Pool<Sqlite>, channel: Channel, config: PlayoutConfig) -> Self {
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
            decoder: Arc::new(Mutex::new(None)),
            encoder: Arc::new(Mutex::new(None)),
            ingest: Arc::new(Mutex::new(None)),
            ingest_stdout: Arc::new(Mutex::new(None)),
            ingest_is_alive: Arc::new(AtomicBool::new(false)),
            is_processing: Arc::new(AtomicBool::new(false)),
            filter_chain: None,
            current_date: Arc::new(Mutex::new(String::new())),
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

    pub async fn start(&self) -> Result<(), ServiceError> {
        if self.is_alive.swap(true, Ordering::SeqCst) {
            return Ok(()); // runs already, don't start multiple instances
        }

        let self_clone = self.clone();
        let channel_id = self.channel.lock().await.id;

        handles::update_player(&self.db_pool, channel_id, true).await?;

        tokio::spawn(async move {
            const MAX_DELAY: Duration = Duration::from_secs(180);
            let mut elapsed = Duration::from_secs(5);
            let mut retry_delay = Duration::from_millis(500);

            while self_clone.channel.lock().await.active {
                self_clone.is_alive.store(true, Ordering::SeqCst);
                self_clone.list_init.store(true, Ordering::SeqCst);

                let timer = Instant::now();

                if let Err(e) = run_channel(self_clone.clone()).await {
                    trace!(
                        "Runtime has {} active tasks",
                        tokio::runtime::Handle::current()
                            .metrics()
                            .num_alive_tasks()
                    );

                    if let Err(e) = self_clone.stop_all(false).await {
                        error!(target: Target::all(), channel = channel_id; "Failed to stop channel <yellow>{channel_id}</>: {e}");
                        break;
                    }

                    if timer.elapsed() < elapsed {
                        elapsed += retry_delay;
                        retry_delay = cmp::min(retry_delay * 2, MAX_DELAY);
                    } else {
                        elapsed = Duration::from_secs(5);
                        retry_delay = Duration::from_secs(1);
                    }

                    let retry_msg = if self_clone.channel.lock().await.active {
                        format!("Retry in <yellow>{}</> seconds", retry_delay.as_secs())
                    } else {
                        "Stop playout!".to_string()
                    };

                    error!(target: Target::all(), channel = channel_id; "Run channel <yellow>{channel_id}</> failed: {e} | {retry_msg}");

                    sleep(retry_delay).await;
                }
            }

            trace!("Async start done");
        });

        Ok(())
    }

    pub async fn foreground_start(&self, index: usize) -> Result<(), ServiceError> {
        if self.is_alive.swap(true, Ordering::SeqCst) {
            return Ok(()); // runs already, don't start multiple instances
        }

        self.is_alive.store(true, Ordering::SeqCst);
        self.list_init.store(true, Ordering::SeqCst);

        let self_clone = self.clone();
        let channel_id = self.channel.lock().await.id;

        handles::update_player(&self.db_pool, channel_id, true).await?;

        if index + 1 == ARGS.channel.clone().unwrap_or_default().len() {
            run_channel(self_clone).await?;
        } else {
            tokio::spawn(async move {
                if let Err(e) = run_channel(self_clone).await {
                    error!(target: Target::all(), channel = channel_id; "Run channel <yellow>{channel_id}</> failed: {e}");
                };
            });
        }

        Ok(())
    }

    pub async fn stop(&self, unit: ProcessUnit) -> Result<(), ServiceError> {
        match unit {
            Decoder => {
                if let Some(proc) = self.decoder.lock().await.as_mut() {
                    proc.kill()
                        .await
                        .map_err(|e| ServiceError::Conflict(format!("Decoder: {e}")))?;
                }
            }
            Encoder => {
                if let Some(proc) = self.encoder.lock().await.as_mut() {
                    proc.kill()
                        .await
                        .map_err(|e| ServiceError::Conflict(format!("Encoder: {e}")))?;
                }
            }
            Ingest => {
                if let Some(proc) = self.ingest.lock().await.as_mut() {
                    proc.kill()
                        .await
                        .map_err(|e| ServiceError::Conflict(format!("Ingest: {e}")))?;
                }
            }
        }

        self.wait(unit).await?;

        Ok(())
    }

    /// Wait for process to proper close.
    /// This prevents orphaned/zombi processes in system
    pub async fn wait(&self, unit: ProcessUnit) -> Result<(), ServiceError> {
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
                    Err(e) => return Err(ServiceError::Conflict(format!("{unit}: {e}"))),
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
            if self.is_alive.load(Ordering::SeqCst) {
                debug!(target: Target::all(), channel = channel_id; "Deactivate playout and stop all child processes from channel: <yellow>{channel_id}</>");
            }

            if let Err(e) = handles::update_player(&self.db_pool, channel_id, false).await {
                error!(target: Target::all(), channel = channel_id; "Player status cannot be written: {e}");
            };
        } else {
            debug!(target: Target::all(), channel = channel_id; "Stop all child processes from channel: <yellow>{channel_id}</>");
        }

        self.is_alive.store(false, Ordering::SeqCst);
        self.ingest_is_alive.store(false, Ordering::SeqCst);

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
    pub managers: Vec<ChannelManager>,
}

impl ChannelController {
    pub fn new() -> Self {
        Self { managers: vec![] }
    }

    pub fn add(&mut self, manager: ChannelManager) {
        self.managers.push(manager);
    }

    pub async fn get(&self, id: i32) -> Option<ChannelManager> {
        for manager in &self.managers {
            if manager.channel.lock().await.id == id {
                return Some(manager.clone());
            }
        }

        None
    }

    pub async fn remove(&mut self, channel_id: i32) {
        let mut indices = Vec::new();

        for (i, manager) in self.managers.iter().enumerate() {
            let channel = manager.channel.lock().await;
            if channel.id == channel_id {
                indices.push(i);
            }
        }

        indices.reverse();

        for i in indices {
            self.managers.remove(i);
        }
    }

    pub fn run_count(&self) -> usize {
        self.managers
            .iter()
            .filter(|manager| manager.is_alive.load(Ordering::SeqCst))
            .count()
    }
}

async fn run_channel(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = manager.config.lock().await.clone();
    let filler_list = manager.filler_list.clone();
    let channel_id = config.general.channel_id;

    drain_hls_path(&config.channel.public).await?;

    debug!(target: Target::all(), channel = channel_id; "Start ffplayout v{VERSION}, channel: <yellow>{channel_id}</>");

    // Fill filler list, can also be a single file.
    // INFO: Was running in a thread, but when it runs in a tokio task and
    // after start a filler is needed, the first one will be ignored because the list is not filled.

    if filler_list.lock().await.is_empty() {
        fill_filler_list(&config, Some(filler_list.clone())).await;
    }

    player(manager).await
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
