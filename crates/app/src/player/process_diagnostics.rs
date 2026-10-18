//! Process measurements for long-running playout investigations.

use std::{
    collections::HashSet,
    fs,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ff_engine::PlaybackControl;
use log::debug;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, Interval, MissedTickBehavior, interval_at},
};

use crate::{player::utils::Media, utils::config::ProcessMode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SourceKind {
    #[default]
    Playlist,
    Folder,
    Filler,
    Ingest,
}

impl SourceKind {
    fn for_clip(mode: &ProcessMode, media: &Media) -> Self {
        if media.is_placeholder || media.source.is_empty() {
            Self::Filler
        } else {
            match mode {
                ProcessMode::Playlist => Self::Playlist,
                ProcessMode::Folder => Self::Folder,
            }
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Playlist => "playlist",
            Self::Folder => "folder",
            Self::Filler => "filler",
            Self::Ingest => "ingest",
        }
    }
}

const SAMPLE_INTERVAL: Duration = Duration::from_secs(5 * 60);

fn sampling_interval() -> Interval {
    let mut interval = interval_at(Instant::now() + SAMPLE_INTERVAL, SAMPLE_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    interval
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ProportionalMemory {
    total: Option<u64>,
    anonymous: Option<u64>,
    file: Option<u64>,
}

#[derive(Default)]
struct Measurements {
    system: System,
    clips: u64,
    previous_rss: Option<u64>,
    previous_cpu: Option<(Instant, u64)>,
    source: SourceKind,
}

struct Snapshot {
    cpu_percent: Option<f64>,
    cpu_window: Option<Duration>,
    rss: u64,
    delta: Option<i128>,
    pss: ProportionalMemory,
    threads: Option<usize>,
    uptime: u64,
    clips: u64,
    source: SourceKind,
}

enum DiagnosticEvent {
    Periodic,
    Live(bool, Option<i32>),
    Clip(SourceKind, bool, Option<i32>, oneshot::Sender<()>),
}

async fn next_event(
    live: &mut mpsc::UnboundedReceiver<(bool, Option<i32>)>,
    clips: &mut mpsc::UnboundedReceiver<DiagnosticEvent>,
    interval: &mut Interval,
) -> Option<DiagnosticEvent> {
    // Live transitions already queued by the engine precede the next clip.
    tokio::select! {
        biased;
        event = live.recv() => event.map(|(active, listener)| DiagnosticEvent::Live(active, listener)),
        event = clips.recv() => event,
        _ = interval.tick() => Some(DiagnosticEvent::Periodic),
    }
}

pub(super) struct ProcessDiagnostics {
    control: PlaybackControl,
    clips: mpsc::UnboundedSender<DiagnosticEvent>,
    live_observer: JoinHandle<()>,
}

impl ProcessDiagnostics {
    pub(super) fn new(channel: i32, control: PlaybackControl) -> Self {
        let measurements = Arc::new(Mutex::new(Measurements::default()));
        let mut live = control.subscribe_live_status();
        let (clips, mut clip_events) = mpsc::unbounded_channel();
        let periodic_control = control.clone();
        let live_observer = tokio::spawn(async move {
            let mut interval = sampling_interval();

            while let Some(event) = next_event(&mut live, &mut clip_events, &mut interval).await {
                let (event, source, active, listener, completed) = match event {
                    DiagnosticEvent::Periodic => {
                        let (active, listener) = periodic_control.live_status();

                        ("periodic", None, active, listener, None)
                    }
                    DiagnosticEvent::Live(active, listener) => {
                        let event = if active { "live_start" } else { "live_end" };

                        (event, None, active, listener, None)
                    }
                    DiagnosticEvent::Clip(source, active, listener, completed) => (
                        "clip_boundary",
                        Some(source),
                        active,
                        listener,
                        Some(completed),
                    ),
                };
                sample(
                    channel,
                    Arc::clone(&measurements),
                    event,
                    source,
                    active,
                    listener,
                )
                .await;

                if let Some(completed) = completed {
                    let _ = completed.send(());
                }
            }
        });

        Self {
            control,
            clips,
            live_observer,
        }
    }

    pub(super) async fn clip_boundary(&self, mode: &ProcessMode, media: &Media) {
        let (active, listener) = self.control.live_status();
        let (completed, completion) = oneshot::channel();
        let event = DiagnosticEvent::Clip(
            SourceKind::for_clip(mode, media),
            active,
            listener,
            completed,
        );

        if self.clips.send(event).is_ok() {
            // Playback starts only after preceding events and this record are logged.
            let _ = completion.await;
        }
    }
}

impl Drop for ProcessDiagnostics {
    fn drop(&mut self) {
        self.live_observer.abort();
    }
}

async fn sample(
    channel: i32,
    measurements: Arc<Mutex<Measurements>>,
    event: &'static str,
    next_source: Option<SourceKind>,
    active: bool,
    listener: Option<i32>,
) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    let result = tokio::task::spawn_blocking(move || {
        let pid = get_current_pid().ok()?;
        let mut state = measurements.lock().unwrap_or_else(PoisonError::into_inner);
        state.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing()
                .with_memory()
                .with_tasks()
                .with_cpu(),
        );
        let process = state.system.process(pid)?;
        let sampled_at = Instant::now();
        let cpu_ms = process.accumulated_cpu_time();
        let cpu = state.previous_cpu.and_then(|(previous_at, previous_ms)| {
            let window = sampled_at.saturating_duration_since(previous_at);

            cpu_percent(previous_ms, cpu_ms, window).map(|percent| (percent, window))
        });
        let rss = process.memory();
        let uptime = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs()
            .saturating_sub(process.start_time());
        // sysinfo excludes the main thread from its Linux task list; other
        // supported platforms currently return None.
        let threads = thread_count(process.tasks());
        let delta = state
            .previous_rss
            .map(|previous| i128::from(rss) - i128::from(previous));
        state.previous_rss = Some(rss);
        state.previous_cpu = Some((sampled_at, cpu_ms));
        if let Some(source) = next_source {
            state.clips += 1;
            state.source = source;
        }
        let pss = if cfg!(target_os = "linux") {
            fs::read_to_string("/proc/self/smaps_rollup")
                .ok()
                .map(|text| parse_pss(&text))
                .unwrap_or_default()
        } else {
            ProportionalMemory::default()
        };

        Some(Snapshot {
            cpu_percent: cpu.map(|(percent, _)| percent),
            cpu_window: cpu.map(|(_, window)| window),
            rss,
            delta,
            pss,
            threads,
            uptime,
            clips: state.clips,
            source: if active {
                SourceKind::Ingest
            } else {
                state.source
            },
        })
    })
    .await;

    match result {
        Ok(Some(snapshot)) => debug!(channel = channel;
            r#"<span class="log-gray">[Process Diagnostics]</span> scope=<span class="log-addr">process</span> channel=<span class="log-number">{channel}</span> event=<span class="log-addr">{event}</span> clip=<span class="log-number">{}</span> uptime_s=<span class="log-number">{}</span> cpu_percent=<span class="log-number">{}</span> cpu_window_s=<span class="log-number">{}</span> rss_bytes=<span class="log-number">{}</span> rss_delta_bytes=<span class="log-number">{}</span> pss_bytes=<span class="log-number">{}</span> pss_anon_bytes=<span class="log-number">{}</span> pss_file_bytes=<span class="log-number">{}</span> threads=<span class="log-number">{}</span> live=<span class="log-addr">{}</span> listener=<span class="log-number">{}</span> source=<span class="log-addr">{}</span>"#,
            snapshot.clips, snapshot.uptime,
            snapshot.cpu_percent.map_or_else(|| "unavailable".to_string(), |value| format!("{value:.2}")),
            snapshot.cpu_window.map_or_else(|| "unavailable".to_string(), |value| format!("{:.3}", value.as_secs_f64())),
            snapshot.rss,
            snapshot.delta.map_or_else(|| "unavailable".to_string(), |value| value.to_string()),
            snapshot.pss.total.map_or_else(|| "unavailable".to_string(), |value| value.to_string()),
            snapshot.pss.anonymous.map_or_else(|| "unavailable".to_string(), |value| value.to_string()),
            snapshot.pss.file.map_or_else(|| "unavailable".to_string(), |value| value.to_string()),
            snapshot.threads.map_or_else(|| "unavailable".to_string(), |value| value.to_string()),
            active, listener.map_or_else(|| "none".to_string(), |value| value.to_string()), snapshot.source.as_str()),
        _ => {
            debug!(channel = channel; r#"<span class="log-gray">[Process Diagnostics]</span> scope=<span class="log-addr">process</span> channel=<span class="log-number">{channel}</span> event=<span class="log-addr">{event}</span> measurement <span class="log-gray">unavailable</span>"#);
        }
    }
}

// CPU time includes all process threads; 100% represents one fully occupied core.
fn cpu_percent(previous_ms: u64, current_ms: u64, window: Duration) -> Option<f64> {
    let elapsed_cpu_ms = current_ms.checked_sub(previous_ms)?;
    if window.is_zero() {
        return None;
    }

    Some(elapsed_cpu_ms as f64 / (window.as_secs_f64() * 1000.0) * 100.0)
}

fn thread_count(tasks: Option<&HashSet<Pid>>) -> Option<usize> {
    tasks.map(|tasks| tasks.len() + 1)
}

fn parse_pss(text: &str) -> ProportionalMemory {
    ProportionalMemory {
        total: parse_kib_field(text, "Pss:"),
        anonymous: parse_kib_field(text, "Pss_Anon:"),
        file: parse_kib_field(text, "Pss_File:"),
    }
}

fn parse_kib_field(text: &str, name: &str) -> Option<u64> {
    let line = text.lines().find(|line| line.starts_with(name))?;
    let mut fields = line.split_whitespace();
    fields.next()?;
    let value = fields.next()?.parse::<u64>().ok()?;
    if fields.next()? != "kB" {
        return None;
    }

    value.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::{
        DiagnosticEvent, Media, ProcessMode, ProportionalMemory, SAMPLE_INTERVAL, SourceKind,
        cpu_percent, next_event, parse_pss, sampling_interval, thread_count,
    };
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot};

    #[test]
    fn cpu_usage_uses_interval_time_and_all_cores() {
        assert_eq!(cpu_percent(1000, 4000, Duration::from_secs(2)), Some(150.0));
        assert_eq!(cpu_percent(1000, 1000, Duration::from_secs(2)), Some(0.0));
        assert_eq!(cpu_percent(1000, 999, Duration::from_secs(2)), None);
        assert_eq!(cpu_percent(1000, 1001, Duration::ZERO), None);
    }

    #[tokio::test]
    async fn pending_live_events_precede_next_clip_and_its_completion() {
        let (live_sender, mut live) = mpsc::unbounded_channel();
        let (clip_sender, mut clips) = mpsc::unbounded_channel();
        let (completed, mut completion) = oneshot::channel();
        let mut interval = sampling_interval();
        interval.reset_immediately();
        clip_sender
            .send(DiagnosticEvent::Clip(
                SourceKind::Filler,
                false,
                None,
                completed,
            ))
            .unwrap();
        live_sender.send((true, Some(42))).unwrap();
        live_sender.send((false, Some(42))).unwrap();

        assert!(matches!(
            next_event(&mut live, &mut clips, &mut interval).await,
            Some(DiagnosticEvent::Live(true, Some(42)))
        ));
        assert!(matches!(
            next_event(&mut live, &mut clips, &mut interval).await,
            Some(DiagnosticEvent::Live(false, Some(42)))
        ));
        assert!(completion.try_recv().is_err());

        let Some(DiagnosticEvent::Clip(SourceKind::Filler, false, None, completed)) =
            next_event(&mut live, &mut clips, &mut interval).await
        else {
            panic!("expected the next clip after both live transitions");
        };
        completed.send(()).unwrap();
        completion.await.unwrap();
        assert!(matches!(
            next_event(&mut live, &mut clips, &mut interval).await,
            Some(DiagnosticEvent::Periodic)
        ));
    }

    #[tokio::test]
    async fn periodic_sampling_waits_five_minutes_before_first_tick() {
        let mut interval = sampling_interval();
        assert_eq!(interval.period(), SAMPLE_INTERVAL);
        assert_eq!(SAMPLE_INTERVAL.as_secs(), 300);
        tokio::select! {
            biased;
            _ = interval.tick() => panic!("periodic sampling must not tick immediately"),
            () = std::future::ready(()) => {}
        }
    }

    #[test]
    fn task_count_includes_main_thread_and_preserves_unavailable() {
        use std::collections::HashSet;
        use sysinfo::Pid;

        assert_eq!(thread_count(None), None);
        assert_eq!(thread_count(Some(&HashSet::new())), Some(1));
        let workers = HashSet::from([Pid::from_u32(2), Pid::from_u32(3)]);
        assert_eq!(thread_count(Some(&workers)), Some(3));
    }

    #[test]
    fn clip_source_kind_uses_mode_and_placeholder_status() {
        let mut media = Media {
            source: "private/path/clip.mp4".into(),
            ..Media::default()
        };
        assert_eq!(
            SourceKind::for_clip(&ProcessMode::Playlist, &media),
            SourceKind::Playlist
        );
        assert_eq!(
            SourceKind::for_clip(&ProcessMode::Folder, &media),
            SourceKind::Folder
        );
        media.is_placeholder = true;
        assert_eq!(
            SourceKind::for_clip(&ProcessMode::Playlist, &media),
            SourceKind::Filler
        );
        media.is_placeholder = false;
        media.source.clear();
        assert_eq!(
            SourceKind::for_clip(&ProcessMode::Folder, &media),
            SourceKind::Filler
        );
        assert_eq!(SourceKind::Ingest.as_str(), "ingest");
    }

    #[test]
    fn missing_or_invalid_pss_components_do_not_hide_valid_fields() {
        assert_eq!(parse_pss(""), ProportionalMemory::default());
        assert_eq!(
            parse_pss("Pss: 12 kB\nPss_Anon: x kB\nPss_File: 18446744073709551615 kB\n"),
            ProportionalMemory {
                total: Some(12288),
                ..ProportionalMemory::default()
            }
        );
        assert_eq!(
            parse_pss("Pss_Anon: 0 kB\nPss_File: 2 MB\n"),
            ProportionalMemory {
                anonymous: Some(0),
                ..ProportionalMemory::default()
            }
        );
    }

    #[test]
    fn pss_uses_the_exact_field_and_converts_kib_to_bytes() {
        assert_eq!(
            parse_pss("Rss: 42 kB\nPss: 123 kB\nPss_Anon: 10 kB\nPss_File: 2 kB\n"),
            ProportionalMemory {
                total: Some(125952),
                anonymous: Some(10240),
                file: Some(2048),
            }
        );
        for invalid in [
            "Pss_Anon: 10 kB",
            "Pss: x kB",
            "Pss: 1 MB",
            "Pss: 18446744073709551615 kB",
        ] {
            assert_eq!(parse_pss(invalid).total, None);
        }
    }
}
