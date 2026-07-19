use std::{
    cmp, fmt,
    path::Path,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use ff_engine::{AudioEffectsControl, AudioLevel, PlaybackControl, TextOverlayState};
use log::*;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio::{
    process::{Child, ChildStdout},
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{Duration, Instant, sleep},
};
use tokio_util::sync::CancellationToken;

use crate::{
    ARGS,
    db::{handles, models::Channel},
    file::{init_storage, local::LocalStorage},
    player::{output::player, utils::Media},
    utils::{config::PlayoutConfig, errors::ServiceError, logging::Target, system::SystemStat},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SUPERVISOR_STOP_TIMEOUT: Duration = Duration::from_secs(15);

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
    pub id: i32,
    pub db_pool: Pool<Sqlite>,
    pub config: Arc<RwLock<PlayoutConfig>>,
    pub channel: Arc<Mutex<Channel>>,
    pub decoder: Arc<Mutex<Option<Child>>>,
    pub encoder: Arc<Mutex<Option<Child>>>,
    pub ingest: Arc<Mutex<Option<Child>>>,
    pub ingest_reader: Arc<Mutex<Option<ChildStdout>>>,
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
    pub storage: LocalStorage,
    pub supervisor_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub validation_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub metrics_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub supervisor_token: Arc<Mutex<Option<CancellationToken>>>,
    pub validation_token: Arc<Mutex<Option<CancellationToken>>>,
    pub metrics_token: Arc<Mutex<Option<CancellationToken>>>,
    pub task_generation: Arc<AtomicUsize>,
    pub audio_effects: AudioEffectsControl,
    pub audio_level: Arc<StdMutex<Option<AudioLevel>>>,
    pub text_overlay: TextOverlayState,
    pub playback_control: Arc<Mutex<PlaybackControl>>,
    pub shutdown: CancellationToken,
    pub system: SystemStat,
}

impl ChannelManager {
    pub async fn new(
        db_pool: Pool<Sqlite>,
        channel: Channel,
        config: PlayoutConfig,
        shutdown: CancellationToken,
        system: SystemStat,
    ) -> Self {
        let channel_extensions = channel.extra_extensions.clone();
        let mut extensions = config.storage.extensions.clone();
        let mut extra_extensions = channel_extensions
            .split(',')
            .map(Into::into)
            .collect::<Vec<String>>();

        extensions.append(&mut extra_extensions);

        let storage = init_storage(config.channel.storage.clone(), extensions).await;
        let audio_effects = AudioEffectsControl::new(config.processing.volume).unwrap_or_default();
        let text_overlay = TextOverlayState::default();

        Self {
            id: channel.id,
            db_pool,
            is_alive: Arc::new(AtomicBool::new(false)),
            config: Arc::new(RwLock::new(config)),
            channel: Arc::new(Mutex::new(channel)),
            list_init: Arc::new(AtomicBool::new(true)),
            current_media: Arc::new(Mutex::new(None)),
            current_list: Arc::new(Mutex::new(vec![Media::default()])),
            filler_list: Arc::new(Mutex::new(vec![])),
            current_index: Arc::new(AtomicUsize::new(0)),
            filler_index: Arc::new(AtomicUsize::new(0)),
            decoder: Arc::new(Mutex::new(None)),
            encoder: Arc::new(Mutex::new(None)),
            ingest: Arc::new(Mutex::new(None)),
            ingest_reader: Arc::new(Mutex::new(None)),
            ingest_is_alive: Arc::new(AtomicBool::new(false)),
            is_processing: Arc::new(AtomicBool::new(false)),
            filter_chain: None,
            current_date: Arc::new(Mutex::new(String::new())),
            storage,
            supervisor_handle: Arc::new(Mutex::new(None)),
            validation_handle: Arc::new(Mutex::new(None)),
            metrics_handle: Arc::new(Mutex::new(None)),
            supervisor_token: Arc::new(Mutex::new(None)),
            validation_token: Arc::new(Mutex::new(None)),
            metrics_token: Arc::new(Mutex::new(None)),
            task_generation: Arc::new(AtomicUsize::new(0)),
            audio_effects,
            audio_level: Arc::new(StdMutex::new(None)),
            text_overlay,
            playback_control: Arc::new(Mutex::new(PlaybackControl::default())),
            shutdown,
            system,
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

        let channel_storage = channel.storage.clone();
        let channel_extensions = channel.extra_extensions.clone();

        drop(channel);

        let s_path = Path::new(&channel_storage);
        let mut extensions = self.config.read().await.storage.extensions.clone();
        let mut extra_extensions = channel_extensions
            .split(',')
            .map(Into::into)
            .collect::<Vec<String>>();

        extensions.append(&mut extra_extensions);
        *self.storage.root.write().await = s_path.to_path_buf();
        *self.storage.extensions.write().await = extensions;
    }

    pub async fn update_config(&self, new_config: PlayoutConfig) {
        let mut config = self.config.write().await;
        *config = new_config;
    }

    pub async fn start(&self) -> Result<(), ServiceError> {
        if self.is_alive.swap(true, Ordering::SeqCst) {
            return Ok(()); // runs already, don't start multiple instances
        }

        if let Err(error) = handles::update_player(&self.db_pool, self.id, true).await {
            self.is_alive.store(false, Ordering::SeqCst);
            return Err(error.into());
        }

        self.abort_supervisor().await;
        self.spawn_dev_metrics_snapshot().await;

        let generation = self.next_task_generation();
        self.log_dev_task("supervisor", "start", generation).await;
        let token = Self::replace_token(&self.supervisor_token).await;
        let self_clone = self.clone();
        let channel_id = self.id;

        let handle = tokio::spawn(Box::pin(supervisor_loop(
            self_clone, token, channel_id, generation,
        )));

        *self.supervisor_handle.lock().await = Some(handle);

        Ok(())
    }

    pub async fn foreground_start(&self, index: usize) -> Result<(), ServiceError> {
        if self.is_alive.swap(true, Ordering::SeqCst) {
            return Ok(()); // runs already, don't start multiple instances
        }

        if let Err(error) = handles::update_player(&self.db_pool, self.id, true).await {
            self.is_alive.store(false, Ordering::SeqCst);
            return Err(error.into());
        }

        self.list_init.store(true, Ordering::SeqCst);

        self.abort_supervisor().await;
        self.spawn_dev_metrics_snapshot().await;

        let generation = self.next_task_generation();
        self.log_dev_task("supervisor", "start", generation).await;
        let token = Self::replace_token(&self.supervisor_token).await;
        let self_clone = self.clone();
        let channel_id = self.id;

        if index + 1 == ARGS.channel.clone().unwrap_or_default().len() {
            let result = run_channel(self_clone).await;
            self.is_alive.store(false, Ordering::SeqCst);
            result?;
        } else {
            let handle = tokio::spawn(async move {
                if token.is_cancelled() {
                    self_clone.is_alive.store(false, Ordering::SeqCst);
                    return;
                }

                if let Err(e) = run_channel(self_clone.clone()).await {
                    error!(target: Target::All.as_str(), channel = channel_id; "Run channel <span class=\"log-number\">{channel_id}</span> failed: {e}");
                };
                self_clone.is_alive.store(false, Ordering::SeqCst);
            });

            *self.supervisor_handle.lock().await = Some(handle);
        }

        Ok(())
    }

    fn next_task_generation(&self) -> usize {
        self.task_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    async fn log_dev_task(&self, task: &str, event: &str, generation: usize) {
        if cfg!(feature = "dev-metrics") {
            debug!(channel = self.id; "<span class=\"log-gray\">[Dev Metrics]</span> task=<span class=\"log-addr\">{task}</span> event=<span class=\"log-addr\">{event}</span> generation=<span class=\"log-number\">{generation}</span>");
        }
    }

    async fn replace_token(slot: &Arc<Mutex<Option<CancellationToken>>>) -> CancellationToken {
        let token = CancellationToken::new();
        let mut guard = slot.lock().await;

        if let Some(old_token) = guard.replace(token.clone()) {
            old_token.cancel();
        }

        token
    }

    async fn spawn_dev_metrics_snapshot(&self) {
        self.stop_task("metrics", &self.metrics_handle, &self.metrics_token)
            .await;

        if !cfg!(feature = "dev-metrics") {
            return;
        }

        let generation = self.next_task_generation();
        self.log_dev_task("metrics", "start", generation).await;
        let token = Self::replace_token(&self.metrics_token).await;
        let manager = self.clone();
        let system = self.system.clone();
        let handle = tokio::spawn(Box::pin(metrics_snapshot_loop(
            manager, system, token, generation,
        )));

        *self.metrics_handle.lock().await = Some(handle);
    }

    async fn stop_task(
        &self,
        task_name: &str,
        handle: &Arc<Mutex<Option<JoinHandle<()>>>>,
        token: &Arc<Mutex<Option<CancellationToken>>>,
    ) {
        let had_token = if let Some(token) = token.lock().await.take() {
            token.cancel();
            true
        } else {
            false
        };

        if had_token {
            self.log_dev_task(task_name, "cancel_requested", 0).await;
        }

        if let Some(mut task) = handle.lock().await.take() {
            tokio::select! {
                _ = &mut task => {
                    self.log_dev_task(task_name, "joined", 0).await;
                }
                _ = sleep(Duration::from_secs(2)) => {
                    self.log_dev_task(task_name, "abort_fallback", 0).await;
                    task.abort();
                    let _ = task.await;
                }
            }
        }
    }

    pub async fn abort_supervisor(&self) {
        self.stop_task(
            "supervisor",
            &self.supervisor_handle,
            &self.supervisor_token,
        )
        .await;
    }

    /// Wait for the active playout run to return and release its engine resources.
    ///
    /// `stop_all` requests that the current clip is skipped before this method
    /// is called. Avoid cancelling the supervisor up front: doing so can abort
    /// `player` before it reaches `AsyncPlayout::finish`.
    pub async fn stop_supervisor(&self) {
        let task = self.supervisor_handle.lock().await.take();

        if let Some(mut task) = task {
            match tokio::time::timeout(SUPERVISOR_STOP_TIMEOUT, &mut task).await {
                Ok(_) => {
                    self.log_dev_task("supervisor", "joined", 0).await;
                }
                Err(_) => {
                    warn!(channel = self.id;
                        "Playout did not stop within {} seconds; aborting supervisor task",
                        SUPERVISOR_STOP_TIMEOUT.as_secs()
                    );
                    task.abort();
                    let _ = task.await;
                }
            }
        }

        if let Some(token) = self.supervisor_token.lock().await.take() {
            token.cancel();
        }
    }

    pub async fn stop_validation(&self) {
        self.stop_task("validator", &self.validation_handle, &self.validation_token)
            .await;
    }

    pub async fn stop_dev_metrics_snapshot(&self) {
        self.stop_task("metrics", &self.metrics_handle, &self.metrics_token)
            .await;
    }

    pub async fn spawn_validation(
        &self,
        config: PlayoutConfig,
        current_list: Arc<Mutex<Vec<Media>>>,
        playlist: crate::player::utils::JsonPlaylist,
        is_alive: Arc<AtomicBool>,
    ) {
        self.stop_validation().await;

        let generation = self.next_task_generation();
        self.log_dev_task("validator", "start", generation).await;
        let token = Self::replace_token(&self.validation_token).await;
        let manager = self.clone();
        let handle = tokio::spawn(async move {
            crate::player::utils::json_validate::validate_playlist(
                config,
                current_list,
                playlist,
                is_alive,
                token.clone(),
            )
            .await;

            let event = if token.is_cancelled() {
                "done_cancelled"
            } else {
                "done"
            };
            manager.log_dev_task("validator", event, generation).await;
        });

        *self.validation_handle.lock().await = Some(handle);
    }

    pub async fn stop(&self, unit: ProcessUnit) {
        self.storage.stop_watch().await;

        let child = match unit {
            Decoder => &self.decoder,
            Encoder => &self.encoder,
            Ingest => &self.ingest,
        };

        if let Some(p) = child.lock().await.as_mut()
            && let Err(e) = p.kill().await
            && !e.to_string().contains("exited process")
        {
            error!("Failed to kill {unit} process: {e}");
        }

        self.wait(unit).await;
    }

    /// Wait for process to proper close.
    /// This prevents orphaned/zombi processes in system
    pub async fn wait(&self, unit: ProcessUnit) {
        let child = match unit {
            Decoder => &self.decoder,
            Encoder => &self.encoder,
            Ingest => &self.ingest,
        };

        if let Some(proc) = child.lock().await.as_mut() {
            let mut counter = 0;
            loop {
                match proc.try_wait() {
                    Ok(Some(_)) => {
                        break;
                    }
                    Ok(None) => {
                        if counter > 300 {
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                        counter += 1;
                    }
                    Err(e) => {
                        if !e.to_string().contains("exited process") {
                            error!(target: Target::All.as_str(), channel = self.id; "{unit}: {e}");
                        }
                    }
                }
            }
        }

        *child.lock().await = None;

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    /// No matter what is running, terminate them all.
    pub async fn stop_all(&self, permanent: bool) {
        let channel_id = self.id;

        if permanent {
            if self.is_alive.load(Ordering::SeqCst) {
                debug!(target: Target::All.as_str(), channel = channel_id; "Deactivate playout and stop all child processes from channel: <span class=\"log-number\">{channel_id}</span>");
            }

            if let Err(e) = handles::update_player(&self.db_pool, channel_id, false).await {
                error!(target: Target::All.as_str(), channel = channel_id; "Player status cannot be written: {e}");
            };

            self.stop_validation().await;
            self.stop_dev_metrics_snapshot().await;
        } else {
            debug!(target: Target::All.as_str(), channel = channel_id; "Stop all child processes from channel: <span class=\"log-number\">{channel_id}</span>");
        }

        self.is_alive.store(false, Ordering::SeqCst);
        self.ingest_is_alive.store(false, Ordering::SeqCst);
        self.playback_control.lock().await.skip_current();

        for unit in [Decoder, Encoder, Ingest] {
            self.stop(unit).await;
        }
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

    pub fn get(&self, id: i32) -> Option<ChannelManager> {
        self.managers.iter().find(|m| m.id == id).cloned()
    }

    pub fn remove(&mut self, id: i32) {
        let mut indices = Vec::new();

        for (i, manager) in self.managers.iter().enumerate() {
            let channel_id = manager.id;
            if channel_id == id {
                indices.push(i);
            }
        }

        indices.reverse();

        for i in indices {
            self.managers.remove(i);
        }
    }
}

async fn supervisor_loop(
    manager: ChannelManager,
    token: CancellationToken,
    channel_id: i32,
    generation: usize,
) {
    const MAX_DELAY: Duration = Duration::from_secs(180);
    let mut elapsed = Duration::from_secs(5);
    let mut retry_delay = Duration::from_millis(500);

    loop {
        if token.is_cancelled() {
            break;
        }
        let active = {
            let channel = manager.channel.lock().await;
            channel.active
        };

        if !active {
            break;
        }

        manager.is_alive.store(true, Ordering::SeqCst);
        manager.list_init.store(true, Ordering::SeqCst);

        let timer = Instant::now();

        if let Err(e) = run_channel(manager.clone()).await {
            manager.stop_all(false).await;

            let active = {
                let channel = manager.channel.lock().await;
                channel.active
            };

            if !active {
                break;
            }

            if timer.elapsed() < elapsed {
                elapsed += retry_delay;
                retry_delay = cmp::min(retry_delay * 2, MAX_DELAY);
            } else {
                elapsed = Duration::from_secs(5);
                retry_delay = Duration::from_secs(1);
            }

            let retry_msg = format!(
                "Retry in <span class=\"log-number\">{}</span> seconds",
                retry_delay.as_secs()
            );

            error!(target: Target::All.as_str(), channel = channel_id; "Run channel <span class=\"log-number\">{channel_id}</span> failed: {e} | {retry_msg}");

            trace!(
                "Runtime has <span class=\"log-number\">{}</span> active tasks",
                tokio::runtime::Handle::current()
                    .metrics()
                    .num_alive_tasks()
            );

            tokio::select! {
                _ = token.cancelled() => break,
                _ = sleep(retry_delay) => {}
            }
        }
    }

    let event = if token.is_cancelled() {
        "done_cancelled"
    } else {
        "done"
    };
    manager.log_dev_task("supervisor", event, generation).await;
    trace!("Async start done");
}

async fn metrics_snapshot_loop(
    manager: ChannelManager,
    system: SystemStat,
    token: CancellationToken,
    generation: usize,
) {
    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = sleep(Duration::from_secs(180)) => {
                let metrics = tokio::runtime::Handle::current().metrics();
                let (thread_count, rss) = system.process_snapshot().await;
                #[cfg(tokio_unstable)]
                debug!(

                    channel = manager.id;
                    "<span class=\"log-gray\">[Dev Metrics]</span> task=<span class=\"log-addr\">runtime_snapshot</span> event=<span class=\"log-addr\">tick</span> generation=<span class=\"log-number\">{generation}</span> tokio_alive=<span class=\"log-number\">{}</span> tokio_workers=<span class=\"log-number\">{}</span> global_queue_depth=<span class=\"log-number\">{}</span> blocking_queue_depth=<span class=\"log-number\">{}</span> threads=<span class=\"log-number\">{thread_count}</span> rss=<span class=\"log-number\">{rss}</span>",
                    metrics.num_alive_tasks(),
                    metrics.num_workers(),
                    metrics.global_queue_depth(),
                    metrics.blocking_queue_depth(),
                );
                #[cfg(not(tokio_unstable))]
                debug!(

                    channel = manager.id;
                    "<span class=\"log-gray\">[Dev Metrics]</span> task=<span class=\"log-addr\">runtime_snapshot</span> event=<span class=\"log-addr\">tick</span> generation=<span class=\"log-number\">{generation}</span> tokio_alive=<span class=\"log-number\">{}</span> tokio_workers=<span class=\"log-number\">{}</span> global_queue_depth=<span class=\"log-number\">{}</span> threads=<span class=\"log-number\">{thread_count}</span> rss=<span class=\"log-number\">{rss}</span>",
                    metrics.num_alive_tasks(),
                    metrics.num_workers(),
                    metrics.global_queue_depth(),
                );
            }
        }
    }

    let event = if token.is_cancelled() {
        "done_cancelled"
    } else {
        "done"
    };
    manager.log_dev_task("metrics", event, generation).await;
}

async fn run_channel(manager: ChannelManager) -> Result<(), ServiceError> {
    let config = {
        let guard = manager.config.read().await;
        guard.clone()
    };

    let filler_list = manager.filler_list.clone();
    let channel_id = config.general.channel_id;

    debug!(target: Target::All.as_str(), channel = channel_id; "Start ffplayout v{VERSION}, channel: <span class=\"log-number\">{channel_id}</span>");

    let need_fill = {
        let list = filler_list.lock().await;
        list.is_empty()
    };

    if need_fill {
        // Fill filler list, can also be a single file.
        // INFO: Was running in a thread, but when it runs in a tokio task and
        // after start a filler is needed, the first one will be ignored because the list is not filled.
        manager
            .storage
            .fill_filler_list(&config, Some(filler_list.clone()))
            .await;
    }

    // 4. Player starten
    player(manager).await
}
