//! Live-input listeners, arbitration, and receiver state.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, VecDeque},
    error::Error,
    ffi::{CStr, CString},
    fmt, ptr,
    sync::{
        Arc, LazyLock, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use ffmpeg_next::{Dictionary, ffi, format, frame, media};
use log::{debug, error, info, warn};

use crate::{
    PlaybackControl,
    audio_mixer::{LIVE_LATENCY, LiveLoudnessControl, LiveLoudnessMetrics, LiveLoudnessProcessor},
    benchmark::{self, BenchHandle, Stage},
    output::FrameOutput,
    playout::{InputPlaybackOptions, LogoFadePlan, Timeline, play_opened_input},
    utils::{
        config::{OutputConfig, validate_input_protocol_options, validate_live_demuxer_options},
        ffmpeg::{reference_audio_frame, reference_video_frame},
        logging,
    },
};

use super::delay::LiveDelay;
use super::diagnostics::{LiveInputDiagnostics, LiveInputStage};
use super::timestamps::{LateAudioRecovery, LiveTimestampTracker};

pub(crate) use super::playback::LiveOverrideOutput;

pub(super) const LIVE_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const MAX_PENDING_AUDIO_FRAMES: usize = 512;
pub(super) const LIVE_AUDIO_FRAME_SAMPLES: usize = 1024;

// The live decoder supplies stereo planar f32 at the configured sample rate.
pub(super) fn trim_audio_start(input: &frame::Audio, skip: usize) -> Result<frame::Audio> {
    anyhow::ensure!(skip < input.samples(), "cannot trim an entire audio frame");
    let mut result = frame::Audio::new(
        input.format(),
        input.samples() - skip,
        input.channel_layout(),
    );
    result.set_rate(input.rate());
    result.set_pts(input.pts().map(|pts| pts + skip as i64));

    for channel in 0..input.channels() as usize {
        result
            .plane_mut::<f32>(channel)
            .copy_from_slice(&input.plane::<f32>(channel)[skip..]);
    }

    Ok(result)
}
pub(super) const LIVE_IDLE_TIMEOUT: Duration = Duration::from_millis(1500);
const LIVE_WATCHDOG_INTERVAL: Duration = Duration::from_millis(100);
/// Limit wall-clock gap filling when returning from an interrupted live input.
pub(super) const MAX_LIVE_GAP_SECONDS: f64 = 5.0;
/// The live channel carries decoded raw frames (several MB each for video);
/// it must be bounded so a stalled consumer cannot exhaust memory.
const LIVE_CHANNEL_SECONDS: usize = 2;
// The reader can legitimately be this far ahead while the bounded live queue
// drains. Do not synthesize silence during that interval: doing so overlaps
// real audio once it arrives and produces an audible pulsing effect.
pub(super) const LIVE_AUDIO_GRACE_SECONDS: f64 = LIVE_CHANNEL_SECONDS as f64 + 0.5;
// FLV timestamps use millisecond precision. At 48 kHz that can make otherwise
// contiguous AAC frames appear to overlap or have a gap by a few dozen
// samples. Treat deviations below 5 ms as timestamp quantization, while
// preserving real packet loss and discontinuities.
pub(super) const LIVE_AUDIO_PTS_JITTER_SECONDS: f64 = 0.005;
pub(super) const LIVE_SEND_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const LIVE_BACKPRESSURE_LOG_THRESHOLD: Duration = Duration::from_secs(1);
// FFmpeg may need one network polling cycle to observe the interrupt callback
// after an idle timeout. Give the reader a bounded chance to close its input
// and socket before detaching it and starting the next listener.
const LIVE_READER_SHUTDOWN_GRACE: Duration = Duration::from_millis(250);

/// Number of RTMP reader threads that outlived their `abort` signal and are
/// being reaped in the background. Exposed only via log messages for now;
/// see the usage in `run_rtmp_listener` for context.
static STUCK_LIVE_WORKERS: AtomicU64 = AtomicU64::new(0);

const MAX_LIVE_READERS_PER_INPUT: usize = 2;
type ReaderKey = (Option<i32>, String);
#[derive(Default)]
pub(super) struct ReaderState {
    count: usize,
    last_warning: Option<Instant>,
}
pub(super) static LIVE_READERS: LazyLock<Mutex<HashMap<ReaderKey, ReaderState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) struct LiveReaderPermit(ReaderKey);

impl LiveReaderPermit {
    pub(super) fn acquire(key: ReaderKey, abort: &AtomicBool) -> Option<Self> {
        while !abort.load(Ordering::Relaxed) {
            if let Some(permit) = Self::try_acquire(&key) {
                return Some(permit);
            }

            let warn = {
                let mut readers = LIVE_READERS.lock().unwrap_or_else(PoisonError::into_inner);
                readers.get_mut(&key).is_some_and(|state| {
                    if state.count >= MAX_LIVE_READERS_PER_INPUT
                        && state
                            .last_warning
                            .is_none_or(|time| time.elapsed() >= Duration::from_secs(300))
                    {
                        state.last_warning = Some(Instant::now());
                        true
                    } else {
                        false
                    }
                })
            };

            if warn {
                error!(channel = key.0.unwrap_or_default(); "live input blocked: both reader slots are occupied; waiting for a previous reader to exit (a process restart may be required)");
            }
            thread::sleep(LIVE_WATCHDOG_INTERVAL);
        }
        None
    }

    pub(super) fn try_acquire(key: &ReaderKey) -> Option<Self> {
        let mut readers = LIVE_READERS.lock().unwrap_or_else(PoisonError::into_inner);
        let state = readers.entry(key.clone()).or_default();

        if state.count >= MAX_LIVE_READERS_PER_INPUT {
            return None;
        }

        state.count += 1;
        Some(Self(key.clone()))
    }
}

impl Drop for LiveReaderPermit {
    fn drop(&mut self) {
        let mut readers = LIVE_READERS.lock().unwrap_or_else(PoisonError::into_inner);
        let mut recovered = false;

        if let Some(state) = readers.get_mut(&self.0) {
            state.count -= 1;
            recovered = state.last_warning.take().is_some();

            if state.count == 0 {
                readers.remove(&self.0);
            }
        }
        drop(readers);

        if recovered {
            info!(channel = self.0.0.unwrap_or_default(); "live reader capacity recovered; connections can resume");
        }
    }
}

pub struct LiveReceiver {
    pub(super) rx: Receiver<LiveEvent>,
    pub(super) pending_event: Option<LiveEvent>,
    pub(super) delay: LiveDelay,
    pub(super) live_session: Option<crate::LiveSession>,
    pub(super) abort: Arc<AtomicBool>,
    pub(super) channel_id: i32,
    pub(super) fps: u32,
    pub(super) sample_rate: u32,
    pub(super) loudness_control: LiveLoudnessControl,
    pub(super) active: bool,
    pub(super) connecting: bool,
    pub(super) session_id: u64,
    pub(super) session_output_start_seconds: Option<f64>,
    pub(super) session_source_start_seconds: Option<f64>,
    pub(super) video_timestamps: LiveTimestampTracker,
    pub(super) audio_timestamps: LiveTimestampTracker,
    pub(super) audio_recovery: LateAudioRecovery,
    pub(super) audio_output_offset_pts: i64,
    pub(super) pending_audio: VecDeque<frame::Audio>,
    pub(super) pending_audio_samples: usize,
    pub(super) last_media_at: Option<Instant>,
    pub(super) last_audio_at: Option<Instant>,
    pub(super) last_video_frame: Option<frame::Video>,
    pub(super) last_video_output_pts: Option<i64>,
    pub(super) last_audio_output_end_pts: Option<i64>,
    pub(super) file_resume_at_seconds: Option<f64>,
    pub(super) file_resume_shift_seconds: Option<f64>,
    pub(super) returned_to_file: bool,
    pub(super) video_pts: i64,
    pub(super) audio_pts: i64,
    pub(super) source_has_audio: bool,
    pub(super) listener_id: i32,
    pub(super) loudness: Option<LiveLoudnessProcessor>,
    pub(super) benchmark: Arc<Mutex<Option<BenchHandle>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveInputBackend {
    Rtmp,
    Srt,
}

impl fmt::Display for LiveInputBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Rtmp => "RTMP",
            Self::Srt => "SRT",
        })
    }
}

pub fn live_protocol_available(backend: LiveInputBackend) -> bool {
    let name = match backend {
        LiveInputBackend::Rtmp => b"rtmp".as_slice(),
        LiveInputBackend::Srt => b"srt".as_slice(),
    };
    let mut cursor = ptr::null_mut();

    loop {
        let protocol = unsafe { ffi::avio_enum_protocols(&mut cursor, 0) };

        if protocol.is_null() {
            return false;
        }

        if unsafe { CStr::from_ptr(protocol) }.to_bytes() == name {
            return true;
        }
    }
}

#[derive(Clone, Debug)]
pub struct LiveListenerConfig {
    pub id: i32,
    pub priority: i32,
    pub backend: LiveInputBackend,
    pub url: String,
    pub options: BTreeMap<String, String>,
    pub demuxer_options: BTreeMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct LiveEnded;

impl fmt::Display for LiveEnded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("live input ended")
    }
}

impl Error for LiveEnded {}

pub(super) enum LiveEvent {
    Started {
        session_id: u64,
        has_audio: bool,
        listener_id: i32,
    },
    Video(u64, frame::Video),
    Audio(u64, frame::Audio),
    Ended(u64),
}

pub fn spawn_rtmp_listener(url: String, cfg: OutputConfig) -> LiveReceiver {
    spawn_listener(
        LiveListenerConfig {
            id: 0,
            priority: 0,
            backend: LiveInputBackend::Rtmp,
            url,
            options: BTreeMap::new(),
            demuxer_options: BTreeMap::new(),
        },
        cfg,
    )
}

/// Listen on all configured transports while presenting one selected session
/// to the playout timeline. An on-air session is never pre-empted.
pub fn spawn_live_listeners(
    mut inputs: Vec<LiveListenerConfig>,
    cfg: OutputConfig,
) -> LiveReceiver {
    inputs.sort_by_key(|input| (Reverse(input.priority), input.id));

    if inputs.len() == 1 {
        let input = inputs.pop().unwrap();

        return spawn_listener(input, cfg);
    }

    let (tx, rx) = mpsc::sync_channel(live_channel_capacity(cfg.fps));
    let abort = Arc::new(AtomicBool::new(false));
    let benchmark = Arc::new(Mutex::new(None));
    let receiver = LiveReceiver::new(rx, Arc::clone(&abort), Arc::clone(&benchmark), &cfg);
    let listeners = inputs
        .into_iter()
        .map(|input| ListenerSlot {
            receiver: spawn_listener_with_benchmark(
                input,
                cfg.clone(),
                Arc::clone(&benchmark),
                // The relay drains standby inputs continuously. Keep their
                // queues short; the selected output still has its own full
                // two-second channel above for brief playout stalls.
                (cfg.fps as usize / 4).max(4),
            ),
            session: None,
            closed: false,
            pending_audio: VecDeque::new(),
            pending_audio_samples: 0,
        })
        .collect();
    let channel_id = cfg.channel_id.unwrap_or_default();

    thread::spawn(move || relay_live_listeners(listeners, tx, abort, channel_id));

    receiver
}

pub(super) struct ListenerSlot {
    pub(super) receiver: LiveReceiver,
    pub(super) session: Option<(u64, bool, i32)>,
    pub(super) closed: bool,
    pub(super) pending_audio: VecDeque<frame::Audio>,
    pub(super) pending_audio_samples: usize,
}

impl ListenerSlot {
    fn clear_pending_audio(&mut self) {
        self.pending_audio.clear();
        self.pending_audio_samples = 0;
    }

    fn buffer_audio(&mut self, frame: frame::Audio) -> Result<()> {
        let limit = (self.receiver.sample_rate as usize)
            .saturating_mul(LIVE_STARTUP_TIMEOUT.as_secs() as usize);
        let frame = if frame.samples() > limit {
            trim_audio_start(&frame, frame.samples() - limit)?
        } else {
            frame
        };

        while !self.pending_audio.is_empty()
            && (self.pending_audio_samples.saturating_add(frame.samples()) > limit
                || self.pending_audio.len() >= MAX_PENDING_AUDIO_FRAMES)
        {
            let old = self.pending_audio.pop_front().unwrap();
            self.pending_audio_samples -= old.samples();
        }

        self.pending_audio_samples += frame.samples();
        self.pending_audio.push_back(frame);

        Ok(())
    }
}

pub(super) fn relay_live_listeners(
    mut listeners: Vec<ListenerSlot>,
    tx: SyncSender<LiveEvent>,
    abort: Arc<AtomicBool>,
    channel_id: i32,
) {
    let mut selected: Option<(usize, u64, u64)> = None;
    let mut next_session = 0u64;
    let mut pending_output = VecDeque::new();

    while !abort.load(Ordering::Relaxed) {
        if flush_relay_events(&tx, &mut pending_output).is_err() {
            return;
        }

        let mut received = false;

        for (index, listener) in listeners.iter_mut().enumerate() {
            if listener.closed {
                continue;
            }

            // Backpressure must affect only the on-air source. Continue
            // draining standby receivers so their SRT socket buffers do not
            // fill while the output is temporarily blocked.
            if !pending_output.is_empty() && selected.is_some_and(|(active, _, _)| active == index)
            {
                continue;
            }

            // Drain a bounded burst before considering the next priority. A
            // startup audio packet must not hide an already queued first video
            // frame and let a lower-priority listener win instead.
            let burst_limit = live_channel_capacity(listener.receiver.fps).min(512) + 1;

            for _ in 0..burst_limit {
                let event = match listener.receiver.rx.try_recv() {
                    Ok(event) => event,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        listener.closed = true;
                        listener.session = None;
                        listener.clear_pending_audio();

                        if selected.is_some_and(|(active, _, _)| active == index)
                            && let Some((_, _, session_id)) = selected.take()
                        {
                            pending_output.push_back(LiveEvent::Ended(session_id));
                        }

                        break;
                    }
                };
                received = true;

                let forwarded = match event {
                    LiveEvent::Started {
                        session_id,
                        has_audio,
                        listener_id,
                    } => {
                        listener.session = Some((session_id, has_audio, listener_id));
                        listener.clear_pending_audio();
                        None
                    }
                    LiveEvent::Video(session_id, frame) => {
                        if selected.is_none()
                            && pending_output.is_empty()
                            && let Some((candidate, has_audio, listener_id)) = listener.session
                            && candidate == session_id
                        {
                            next_session = next_session.wrapping_add(1);
                            selected = Some((index, session_id, next_session));

                            pending_output.push_back(LiveEvent::Started {
                                session_id: next_session,
                                has_audio,
                                listener_id,
                            });

                            // Start playback before draining older audio. Its
                            // source PTS still aligns the audio afterwards,
                            // while the first visible frame is not delayed by
                            // a long buffered audio-only lead-in.
                            pending_output.push_back(LiveEvent::Video(next_session, frame));

                            for audio in listener.pending_audio.drain(..) {
                                pending_output.push_back(LiveEvent::Audio(next_session, audio));
                            }
                            listener.pending_audio_samples = 0;

                            None
                        } else {
                            selected
                                .filter(|(active, source_session, _)| {
                                    *active == index && *source_session == session_id
                                })
                                .map(|(_, _, relay_session)| LiveEvent::Video(relay_session, frame))
                        }
                    }
                    LiveEvent::Audio(session_id, frame) => {
                        if let Some((_, _, relay_session)) =
                            selected.filter(|(active, source_session, _)| {
                                *active == index && *source_session == session_id
                            })
                        {
                            Some(LiveEvent::Audio(relay_session, frame))
                        } else {
                            if listener
                                .session
                                .is_some_and(|(candidate, _, _)| candidate == session_id)
                                && let Err(error) = listener.buffer_audio(frame)
                            {
                                warn!(channel = channel_id; "unable to buffer live audio: {error:#}");
                            }

                            None
                        }
                    }
                    LiveEvent::Ended(session_id) => {
                        if listener
                            .session
                            .is_some_and(|(candidate, _, _)| candidate == session_id)
                        {
                            listener.session = None;
                            listener.clear_pending_audio();
                        }

                        if selected.is_some_and(|(active, source_session, _)| {
                            active == index && source_session == session_id
                        }) {
                            selected
                                .take()
                                .map(|(_, _, relay_session)| LiveEvent::Ended(relay_session))
                        } else {
                            None
                        }
                    }
                };

                if let Some(event) = forwarded {
                    pending_output.push_back(event);
                }

                if flush_relay_events(&tx, &mut pending_output).is_err() {
                    return;
                }

                if !pending_output.is_empty()
                    && selected.is_some_and(|(active, _, _)| active == index)
                {
                    break;
                }
            }
        }

        if listeners.iter().all(|listener| listener.closed) && pending_output.is_empty() {
            return;
        }

        if !received {
            thread::sleep(LIVE_SEND_RETRY_INTERVAL);
        }
    }
}

fn flush_relay_events(
    tx: &SyncSender<LiveEvent>,
    pending: &mut VecDeque<LiveEvent>,
) -> std::result::Result<(), ()> {
    while let Some(event) = pending.pop_front() {
        match tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(event)) => {
                pending.push_front(event);

                break;
            }
            Err(TrySendError::Disconnected(_)) => return Err(()),
        }
    }

    Ok(())
}

fn spawn_listener(input: LiveListenerConfig, cfg: OutputConfig) -> LiveReceiver {
    let capacity = live_channel_capacity(cfg.fps);
    spawn_listener_with_benchmark(input, cfg, Arc::new(Mutex::new(None)), capacity)
}

fn spawn_listener_with_benchmark(
    input: LiveListenerConfig,
    cfg: OutputConfig,
    benchmark: Arc<Mutex<Option<BenchHandle>>>,
    capacity: usize,
) -> LiveReceiver {
    let (tx, rx) = mpsc::sync_channel(capacity);
    let abort = Arc::new(AtomicBool::new(false));
    let receiver = LiveReceiver::new(rx, Arc::clone(&abort), Arc::clone(&benchmark), &cfg);
    thread::spawn({
        let abort = Arc::clone(&abort);
        let benchmark = Arc::clone(&benchmark);
        move || run_live_listener(input, cfg, tx, abort, benchmark)
    });

    receiver
}

impl LiveReceiver {
    fn new(
        rx: Receiver<LiveEvent>,
        abort: Arc<AtomicBool>,
        benchmark: Arc<Mutex<Option<BenchHandle>>>,
        cfg: &OutputConfig,
    ) -> Self {
        Self {
            rx,
            pending_event: None,
            delay: LiveDelay::new(LIVE_LATENCY, cfg.fps, cfg.sample_rate),
            live_session: None,
            abort,
            channel_id: cfg.channel_id.unwrap_or_default(),
            fps: cfg.fps,
            sample_rate: cfg.sample_rate,
            loudness_control: cfg.live_loudness_control.clone(),
            active: false,
            connecting: false,
            session_id: 0,
            session_output_start_seconds: None,
            session_source_start_seconds: None,
            video_timestamps: LiveTimestampTracker::default(),
            audio_timestamps: LiveTimestampTracker::default(),
            audio_recovery: LateAudioRecovery::default(),
            audio_output_offset_pts: 0,
            pending_audio: VecDeque::new(),
            pending_audio_samples: 0,
            last_media_at: None,
            last_audio_at: None,
            last_video_frame: None,
            last_video_output_pts: None,
            last_audio_output_end_pts: None,
            file_resume_at_seconds: None,
            file_resume_shift_seconds: None,
            returned_to_file: false,
            video_pts: 0,
            audio_pts: 0,
            source_has_audio: false,
            listener_id: 0,
            loudness: None,
            benchmark,
        }
    }
}

impl Drop for LiveReceiver {
    fn drop(&mut self) {
        self.abort.store(true, Ordering::Relaxed);
    }
}

impl LiveReceiver {
    pub(crate) fn set_benchmark(&self, benchmark: Option<BenchHandle>) {
        *self
            .benchmark
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = benchmark;
    }

    pub fn loudness_metrics(&self) -> Option<LiveLoudnessMetrics> {
        self.loudness.as_ref().map(LiveLoudnessProcessor::metrics)
    }

    pub(crate) fn reanchor_timeline(&self, timeline: &mut Timeline) {
        timeline.reanchor(self.video_pts, self.audio_pts);
    }
}

pub(super) struct LiveFrameSender {
    pub(super) tx: SyncSender<LiveEvent>,
    pub(super) session_id: u64,
    pub(super) diagnostics: Arc<LiveInputDiagnostics>,
    pub(super) abort: Arc<AtomicBool>,
    pub(super) listener_abort: Arc<AtomicBool>,
    pub(super) channel_id: i32,
}

impl LiveFrameSender {
    /// Sends a decoded live frame with bounded backpressure. A full queue slows
    /// the RTMP reader instead of dropping frames, but the retry loop keeps
    /// checking abort flags so shutdown/restart cannot hang on a blocked send.
    pub(super) fn send_frame(&mut self, event: LiveEvent) -> Result<()> {
        self.diagnostics.frame_ready();
        benchmark::measure(Stage::LiveQueue, || {
            send_live_event(
                &self.tx,
                event,
                Some(&self.abort),
                &self.listener_abort,
                Some(&self.diagnostics),
                "live frame",
                self.channel_id,
            )
        })
    }
}

impl FrameOutput for LiveFrameSender {
    fn input_diagnostics(&self) -> Option<&LiveInputDiagnostics> {
        Some(&self.diagnostics)
    }

    fn audio_frame_size(&self) -> usize {
        LIVE_AUDIO_FRAME_SAMPLES
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        self.send_frame(LiveEvent::Video(
            self.session_id,
            reference_video_frame(frame)?,
        ))
        .context("failed to send live video frame")
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.send_frame(LiveEvent::Audio(
            self.session_id,
            reference_audio_frame(frame)?,
        ))
        .context("failed to send live audio frame")
    }
}

fn run_live_listener(
    input: LiveListenerConfig,
    cfg: OutputConfig,
    tx: SyncSender<LiveEvent>,
    listener_abort: Arc<AtomicBool>,
    benchmark: Arc<Mutex<Option<BenchHandle>>>,
) {
    let LiveListenerConfig {
        id,
        backend,
        url,
        options,
        demuxer_options,
        ..
    } = input;
    let mut session_id = 0;
    let channel_id = cfg.channel_id.unwrap_or_default();

    if backend == LiveInputBackend::Srt && !logging::srt_log_callback_installed() {
        warn!(channel = channel_id;
            "Live listener #{id}: libsrt log handler unavailable; native SRT warnings may still appear on stderr"
        );
    }

    while !listener_abort.load(Ordering::Relaxed) {
        // The reader owns this permit until it really exits, even after a
        // listener restart. Never accumulate unlimited detached FFmpeg contexts.
        let Some(reader_permit) =
            LiveReaderPermit::acquire((cfg.channel_id, url.clone()), &listener_abort)
        else {
            return;
        };
        let abort = Arc::new(AtomicBool::new(false));

        match logging::with_ingest_logs(cfg.channel_id, id, || {
            open_live_listener(
                backend,
                &url,
                &options,
                &demuxer_options,
                Arc::clone(&abort),
                Arc::clone(&listener_abort),
            )
        }) {
            Ok(ictx) => {
                if listener_abort.load(Ordering::Relaxed) {
                    abort.store(true, Ordering::Relaxed);

                    return;
                }

                session_id += 1;
                info!(channel = channel_id; "Live listener #{id} ({backend}) accepted input");
                let diagnostics = Arc::new(LiveInputDiagnostics::new(id, session_id));
                let watchdog =
                    spawn_live_watchdog(Arc::clone(&diagnostics), Arc::clone(&abort), channel_id);

                if send_live_event(
                    &tx,
                    LiveEvent::Started {
                        session_id,
                        has_audio: ictx.streams().best(media::Type::Audio).is_some(),
                        listener_id: id,
                    },
                    Some(&abort),
                    &listener_abort,
                    None,
                    &format!("live start (listener #{id}, session {session_id})"),
                    channel_id,
                )
                .is_err()
                {
                    abort.store(true, Ordering::Relaxed);
                    let _ = watchdog.join();

                    return;
                }

                let (done_tx, done_rx) = mpsc::sync_channel(1);
                let mut output = LiveFrameSender {
                    tx: tx.clone(),
                    session_id,
                    diagnostics,
                    abort: Arc::clone(&abort),
                    listener_abort: Arc::clone(&listener_abort),
                    channel_id,
                };

                let worker_url = url.clone();
                let worker_cfg = cfg.clone();
                let worker_benchmark = benchmark
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                let worker = thread::spawn(move || {
                    let _reader_permit = reader_permit;

                    if let Some(benchmark) = worker_benchmark {
                        benchmark::activate(benchmark);
                    }

                    let mut timeline = Timeline::new();
                    let playback_control = PlaybackControl::default();
                    let logo_fade_plan = LogoFadePlan::none(timeline.video_pts(), &worker_cfg);
                    let result = logging::with_ingest_logs(worker_cfg.channel_id, id, || {
                        play_opened_input(
                            &worker_url,
                            ictx,
                            &worker_cfg,
                            &mut timeline,
                            &mut output,
                            InputPlaybackOptions {
                                seek_seconds: None,
                                duration_seconds: None,
                                subtitles_media_path: None,
                                logo_fade_plan,
                                playback_control: &playback_control,
                                preserve_source_timestamps: true,
                            },
                            None,
                        )
                    });
                    output.diagnostics.set_stage(LiveInputStage::Finishing);
                    let _ = done_tx.send(result.map_err(|error| format!("{error:#}")));
                });

                let mut worker_finished = false;

                while !abort.load(Ordering::Relaxed) && !listener_abort.load(Ordering::Relaxed) {
                    match done_rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(result) => {
                            worker_finished = true;

                            if let Err(error) = result {
                                error!(channel = channel_id; "Live listener #{id} ({backend}) input failed: {error}");
                            }
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            worker_finished = true;
                            break;
                        }
                    }
                }

                abort.store(true, Ordering::Relaxed);
                let _ = watchdog.join();

                if !worker_finished {
                    worker_finished =
                        wait_for_live_reader_exit(&done_rx, LIVE_READER_SHUTDOWN_GRACE);
                }

                if worker_finished {
                    let _ = worker.join();
                } else {
                    // The interrupt callback only aborts FFmpeg I/O between reads; if the
                    // worker is blocked in a single long-running syscall it may not exit
                    // promptly. Rather than block the listener loop on `worker.join()`,
                    // reap it in the background so a stuck reader is still observed (and
                    // its thread reclaimed) once it eventually unblocks or errors out.
                    let stuck_count = STUCK_LIVE_WORKERS.fetch_add(1, Ordering::Relaxed) + 1;
                    warn!(
                        channel = channel_id;
                        "Live listener #{id} reader did not stop within {} ms; restarting ingest server without waiting ({stuck_count} stuck reader(s) pending cleanup)",
                        LIVE_READER_SHUTDOWN_GRACE.as_millis()
                    );
                    thread::spawn(move || {
                        let _ = worker.join();
                        let remaining = STUCK_LIVE_WORKERS.fetch_sub(1, Ordering::Relaxed) - 1;
                        info!(
                            channel = channel_id;
                            "Previously stuck live input reader exited ({remaining} stuck reader(s) still pending)"
                        );
                    });
                }

                debug!(channel = channel_id; "Live listener #{id} ({backend}) input ended; listening again");

                if send_live_event(
                    &tx,
                    LiveEvent::Ended(session_id),
                    None,
                    &listener_abort,
                    None,
                    &format!("live end (listener #{id}, session {session_id})"),
                    channel_id,
                )
                .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                abort.store(true, Ordering::Relaxed);

                if listener_abort.load(Ordering::Relaxed) {
                    return;
                }
                error!(channel = channel_id; "Live listener #{id} ({backend}) failed: {error:#}; retrying");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

pub(super) fn wait_for_live_reader_exit(
    done_rx: &Receiver<std::result::Result<(), String>>,
    timeout: Duration,
) -> bool {
    matches!(
        done_rx.recv_timeout(timeout),
        Ok(_) | Err(RecvTimeoutError::Disconnected)
    )
}

pub(super) fn live_channel_capacity(fps: u32) -> usize {
    (fps as usize).saturating_mul(LIVE_CHANNEL_SECONDS).max(1)
}

pub(super) fn send_live_event(
    tx: &SyncSender<LiveEvent>,
    mut event: LiveEvent,
    abort: Option<&AtomicBool>,
    listener_abort: &AtomicBool,
    diagnostics: Option<&LiveInputDiagnostics>,
    label: &str,
    channel_id: i32,
) -> Result<()> {
    let mut backpressure_since = None;
    let mut warned = false;
    let previous_stage = diagnostics.map(LiveInputDiagnostics::stage);

    loop {
        match tx.try_send(event) {
            Ok(()) => {
                if let Some(diagnostics) = diagnostics {
                    diagnostics.queue_cleared(previous_stage.unwrap_or_default());
                }

                if warned {
                    debug!(channel = channel_id;
                        "live event channel backpressure cleared; {label} sender blocked for {:.3} s{}",
                        backpressure_since.map_or(0.0, |since: Instant| since.elapsed().as_secs_f64()),
                        diagnostics.map_or_else(String::new, |diagnostics| format!(
                            " (listener #{}, session {})", diagnostics.listener_id, diagnostics.session_id
                        ))
                    );
                }

                return Ok(());
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(anyhow::anyhow!("live event channel disconnected"));
            }
            Err(TrySendError::Full(returned_event)) => {
                // A full internal queue means the source reader is alive but
                // temporarily blocked by the output. Do not let the watchdog
                // mistake this intentional backpressure for a dead publisher.
                if let Some(diagnostics) = diagnostics {
                    diagnostics.queue_heartbeat();
                }

                if abort.is_some_and(|abort| abort.load(Ordering::Relaxed))
                    || listener_abort.load(Ordering::Relaxed)
                {
                    return Err(anyhow::anyhow!(
                        "aborted while waiting to send {label} event"
                    ));
                }

                event = returned_event;
                let now = Instant::now();
                let since = *backpressure_since.get_or_insert(now);

                if !warned && now.duration_since(since) >= LIVE_BACKPRESSURE_LOG_THRESHOLD {
                    warn!(
                        channel = channel_id;
                        "live event channel is full; applying backpressure to {label} sender for {:.3} s{}",
                        since.elapsed().as_secs_f64(),
                        diagnostics.map_or_else(String::new, |diagnostics| format!(
                            " (listener #{}, session {})", diagnostics.listener_id, diagnostics.session_id
                        ))
                    );
                    warned = true;
                }
                thread::sleep(LIVE_SEND_RETRY_INTERVAL);
            }
        }
    }
}

pub(super) fn spawn_live_watchdog(
    diagnostics: Arc<LiveInputDiagnostics>,
    abort: Arc<AtomicBool>,
    channel_id: i32,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !abort.load(Ordering::Relaxed) {
            thread::sleep(LIVE_WATCHDOG_INTERVAL);

            // Shutdown may have started during the sleep. Do not report an
            // idle disconnect for a reader already being closed intentionally.
            if abort.load(Ordering::Relaxed) {
                return;
            }

            let last_activity_ms = diagnostics.last_activity_ms.load(Ordering::Relaxed);
            let frame_seen = diagnostics.frame_seen.load(Ordering::Relaxed);
            let timeout = if frame_seen {
                LIVE_IDLE_TIMEOUT
            } else {
                LIVE_STARTUP_TIMEOUT
            };

            let now = monotonic_millis();
            let idle_ms = now.saturating_sub(last_activity_ms);

            if idle_ms >= timeout.as_millis() as u64 {
                let listener_id = diagnostics.listener_id;

                if frame_seen {
                    info!(channel = channel_id;
                        "Live listener #{listener_id} disconnected or idle; restarting listener (idle={idle_ms} ms, timeout={} ms, {})",
                        timeout.as_millis(), diagnostics.snapshot(now)
                    );
                } else {
                    warn!(channel = channel_id;
                        "Live listener #{listener_id} produced no decodable frames; restarting listener (idle={idle_ms} ms, timeout={} ms, {})",
                        timeout.as_millis(), diagnostics.snapshot(now)
                    );
                }
                abort.store(true, Ordering::Relaxed);

                return;
            }
        }
    })
}

pub(super) fn open_live_listener(
    backend: LiveInputBackend,
    url: &str,
    user_options: &BTreeMap<String, String>,
    demuxer_options: &BTreeMap<String, String>,
    abort: Arc<AtomicBool>,
    listener_abort: Arc<AtomicBool>,
) -> Result<format::context::Input> {
    let scheme = match backend {
        LiveInputBackend::Rtmp => "rtmp",
        LiveInputBackend::Srt => "srt",
    };
    validate_input_protocol_options(scheme, user_options).map_err(anyhow::Error::msg)?;
    validate_live_demuxer_options(scheme, demuxer_options).map_err(anyhow::Error::msg)?;

    let mut options = Dictionary::new();
    for (name, value) in user_options {
        options.set(name, value);
    }

    for (name, value) in demuxer_options {
        if name == "format" {
            continue;
        }

        anyhow::ensure!(
            !user_options.contains_key(name),
            "live input option {name:?} is configured for both protocol and demuxer"
        );
        options.set(name, value);
    }

    match backend {
        LiveInputBackend::Rtmp => {
            options.set("listen", "1");
            options.set("timeout", "0");
            logging::clear_unexpected_rtmp_stream();
        }
        LiveInputBackend::Srt => {
            options.set("mode", "listener");
        }
    }

    let input = open_live_input(
        url,
        move || {
            let interrupted =
                abort.load(Ordering::Relaxed) || listener_abort.load(Ordering::Relaxed);
            if interrupted {
                logging::mark_ingest_interrupted();
            }
            interrupted
        },
        options,
        demuxer_options,
    )
    .with_context(|| format!("failed to listen for {backend} input at {url}"))?;

    if backend != LiveInputBackend::Rtmp {
        return Ok(input);
    }

    if let Some((actual_key, expected_key)) = logging::take_unexpected_rtmp_stream() {
        anyhow::bail!(
            "incoming RTMP stream key {actual_key:?} does not match configured key {expected_key:?}"
        );
    }

    // ffmpeg-next does not expose protocol-private AVOptions. Limit the raw
    // context access to this fallback validation of FFmpeg's RTMP playpath.
    let context = unsafe { input.as_ptr().cast_mut() };

    if let Some(expected_key) = rtmp_stream_key(url)
        && let Some(actual_key) = unsafe { rtmp_context_option(context, "rtmp_playpath") }
        && actual_key != expected_key
    {
        anyhow::bail!(
            "incoming RTMP stream key {actual_key:?} does not match configured key {expected_key:?}"
        );
    }

    Ok(input)
}

fn open_live_input<F>(
    url: &str,
    interrupt_callback: F,
    options: Dictionary,
    demuxer_options: &BTreeMap<String, String>,
) -> Result<format::context::Input>
where
    F: FnMut() -> bool + Send + 'static,
{
    let url = CString::new(url).context("live input URL contains a null byte")?;
    let format_name = demuxer_options
        .get("format")
        .map(|name| CString::new(name.as_str()))
        .transpose()
        .context("live demuxer format contains a null byte")?;
    let input_format = format_name.as_ref().map_or(ptr::null_mut(), |name| unsafe {
        ffi::av_find_input_format(name.as_ptr()).cast_mut()
    });
    anyhow::ensure!(
        format_name.is_none() || !input_format.is_null(),
        "configured live input demuxer is unavailable"
    );

    // Mirror ffmpeg-next's interrupt-aware input opening while allowing an
    // explicit AVInputFormat. Keep the guard alive inside the returned Input.
    unsafe {
        let mut context = ffi::avformat_alloc_context();
        anyhow::ensure!(!context.is_null(), "failed to allocate live input context");
        let interrupt = ffmpeg_next::util::interrupt::new(Box::new(interrupt_callback));
        (*context).interrupt_callback = interrupt.interrupt;
        let mut raw_options = options.disown();
        let result =
            ffi::avformat_open_input(&mut context, url.as_ptr(), input_format, &mut raw_options);
        let unused = Dictionary::own(raw_options);

        if result < 0 {
            if !context.is_null() {
                ffi::avformat_close_input(&mut context);
            }

            return Err(ffmpeg_next::Error::from(result).into());
        }

        if let Some((name, _)) = unused
            .iter()
            .find(|(name, _)| demuxer_options.contains_key(*name))
        {
            ffi::avformat_close_input(&mut context);

            anyhow::bail!("live demuxer option {name:?} was not consumed by FFmpeg");
        }

        let result = ffi::avformat_find_stream_info(context, ptr::null_mut());

        if result < 0 {
            ffi::avformat_close_input(&mut context);

            return Err(ffmpeg_next::Error::from(result).into());
        }

        Ok(format::context::Input::wrap_with_interrupt(
            context,
            interrupt.guard,
        ))
    }
}

fn rtmp_stream_key(url: &str) -> Option<String> {
    let path = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split_once('/')
        .map(|(_, path)| path)?;

    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

unsafe fn rtmp_context_option(ps: *mut ffi::AVFormatContext, name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut value = ptr::null_mut();

    let candidates = [
        ps.cast(),
        (!ps.is_null()).then(|| unsafe { (*ps).pb.cast() })?,
        (!ps.is_null() && !unsafe { (*ps).pb }.is_null()).then(|| unsafe { (*(*ps).pb).opaque })?,
    ];

    for candidate in candidates {
        if candidate.is_null() {
            continue;
        }

        let result = unsafe {
            ffi::av_opt_get(
                candidate,
                name.as_ptr(),
                ffi::AV_OPT_SEARCH_CHILDREN,
                &mut value,
            )
        };

        if result >= 0 && !value.is_null() {
            let option = unsafe { CStr::from_ptr(value.cast()) }
                .to_string_lossy()
                .to_string();
            unsafe { ffi::av_free(value.cast()) };

            return (!option.is_empty()).then_some(option);
        }
    }

    None
}

/// Monotonic millisecond clock used for idle-timeout tracking.
///
/// Uses `Instant` (relative to a fixed process-lifetime epoch) instead of
/// `SystemTime`/`UNIX_EPOCH` so that system clock adjustments (e.g. NTP jumps)
/// cannot cause the live watchdog to misfire.
pub(super) fn monotonic_millis() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_millis() as u64
}
