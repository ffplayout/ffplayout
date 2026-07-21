use std::{
    cell::RefCell,
    collections::VecDeque,
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread as std_thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use cosmic_text::Weight;
use ffmpeg_next::{Rational, Rescale, frame, software::scaling, util::format::pixel::Pixel};
use softbuffer::{Context as SoftbufferContext, Surface};
#[cfg(target_os = "linux")]
use winit::platform::wayland::EventLoopBuilderExtWayland;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    platform::pump_events::EventLoopExtPumpEvents,
    window::{Fullscreen, Window, WindowId},
};

use super::{FrameOutput, PlaybackStopped, vtt};
use crate::{
    analysis::audio_level::{AudioLevelCallback, AudioLevelMeter},
    audio_mixer::{AudioEffectChain, AudioEffectsControl},
    benchmark::{self, BenchHandle, Stage},
    compositor::{
        logo::{LogoOverlay, logo_position},
        text::{TextBitmap, render_wrapped_text_bitmap},
    },
    utils::config::{OutputConfig, RgbaColor},
};

mod audio;
pub(crate) mod thread;

use audio::DesktopAudio;

const AUDIO_CHANNELS: usize = 2;
const AUDIO_PREBUFFER_MS: u64 = 100;
const AUDIO_MAX_QUEUE_MS: u64 = 500;
const AUDIO_MAX_PENDING_MS: u64 = 500;
const AUDIO_DEVICE_BUFFER_SAMPLES: u64 = 1_024;
const VIDEO_PREBUFFER_FRAMES: usize = 6;
const VIDEO_DROP_THRESHOLD_FRAMES: i64 = 3;
const VIDEO_STARVATION_GRACE_FRAMES: i64 = 2;
const SCHEDULER_INTERVAL: Duration = Duration::from_millis(2);
const VIDEO_CHANNEL_CAPACITY: usize = 8;
const AUDIO_CHANNEL_CAPACITY: usize = 32;
const CONTROL_CHANNEL_CAPACITY: usize = 8;
const DESKTOP_VOLUME_STEP: f64 = 0.05;
const DESKTOP_VOLUME_MIN: f64 = 0.0;
const DESKTOP_VOLUME_MAX: f64 = 1.5;
const VOLUME_OVERLAY_DURATION: Duration = Duration::from_millis(900);
const WINDOW_ASPECT_SETTLE: Duration = Duration::from_millis(120);
const SUBTITLE_FONT_SIZE: f32 = 24.0;
const SUBTITLE_FULLSCREEN_FONT_SIZE: f32 = 44.0;
const SUBTITLE_OUTLINE: u32 = 2;
const SUBTITLE_MARGIN_BOTTOM: u32 = 56;
const SUBTITLE_MAX_WIDTH_PERCENT: u32 = 92;

fn video_prebuffer_ready(queue_len: usize, video_finished: bool, force: bool) -> bool {
    force || video_finished || queue_len >= VIDEO_PREBUFFER_FRAMES
}

fn scaled_aspect_dimension(value: u32, numerator: u32, denominator: u32) -> u32 {
    ((u64::from(value) * u64::from(numerator) + (u64::from(denominator) / 2))
        / u64::from(denominator))
    .max(1) as u32
}

fn video_frame_is_too_late(frame_pts: i64, expected_pts: i64, queue_len: usize) -> bool {
    queue_len > 1 && expected_pts.saturating_sub(frame_pts) > VIDEO_DROP_THRESHOLD_FRAMES
}

pub(super) struct DesktopOutput {
    renderer: DesktopRenderer,
    audio_effects: Arc<Mutex<AudioEffectChain>>,
    audio_level_callback: Option<AudioLevelCallback>,
    audio_sample_rate: u32,
}

enum DesktopControlMessage {
    ClipStarted,
    Subtitles(Vec<DesktopSubtitleCue>),
    VideoEnd(Option<i64>),
    VideoFinished,
    ClipFinished,
}

struct DesktopVideoMessage {
    frame: frame::Video,
    logo_opacity: f64,
}

struct DesktopAudioMessage {
    samples: Vec<f32>,
    samples_per_channel: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DesktopDiscontinuity {
    video_pts: i64,
    audio_pts: i64,
}

#[derive(Debug, Clone)]
struct DesktopSubtitleCue {
    start_ms: i64,
    end_ms: i64,
    text: String,
}

pub(crate) struct DesktopFrameSender {
    video_sender: SyncSender<DesktopVideoMessage>,
    audio_sender: SyncSender<DesktopAudioMessage>,
    control_sender: SyncSender<DesktopControlMessage>,
    discontinuity_sender: SyncSender<DesktopDiscontinuity>,
    audio_effects: Arc<Mutex<AudioEffectChain>>,
    audio_level_meter: AudioLevelMeter,
    current_logo_opacity: f64,
}

struct DesktopRenderer {
    audio: DesktopAudio,
    audio_effects_control: AudioEffectsControl,
    video_queue: VecDeque<frame::Video>,
    pending_audio: VecDeque<(Vec<f32>, usize)>,
    pending_audio_samples: u64,
    submitted_audio_samples: u64,
    audio_started: bool,
    sample_rate: u32,
    device_buffer_samples: u64,
    audio_clock: AudioMasterClock,
    video_time_base: Rational,
    video_end_pts: Option<i64>,
    video_finished: bool,
    last_rendered_video_pts: Option<i64>,
    last_video_present: Option<Instant>,
    last_starvation_report: Option<Instant>,
    fps: u32,
    subtitles_enabled: bool,
    subtitles: Vec<DesktopSubtitleCue>,
    active_subtitle_text: Option<String>,
    subtitle_bitmap: Option<RgbaBitmap>,
    logo: Option<DesktopLogo>,
    current_logo_opacity: f64,
    aspect_width: u32,
    aspect_height: u32,
    last_window_size: (u32, u32),
    pending_aspect_resize: Option<(u32, u32, Instant)>,
    volume_overlay_until: Option<Instant>,
    last_video: Option<VideoSurface>,
    frame_converter: DesktopFrameConverter,
}

thread_local! {
    static DESKTOP_WINDOW: RefCell<Option<DesktopWindow>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct RgbaBitmap {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

struct DesktopLogo {
    bitmap: RgbaBitmap,
    position: String,
    opacity: u8,
}

#[derive(Clone)]
struct VideoSurface {
    width: u32,
    height: u32,
    pixels: Arc<[u32]>,
    pts: i64,
}

struct DesktopFrameConverter {
    scaler: Option<scaling::Context>,
    converted: frame::Video,
}

impl DesktopOutput {
    pub(super) fn open(cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            renderer: DesktopRenderer::open(cfg)?,
            audio_effects: Arc::new(Mutex::new(AudioEffectChain::new(
                cfg.audio_effects.clone(),
                cfg.sample_rate,
            ))),
            audio_level_callback: cfg.audio_level_callback.clone(),
            audio_sample_rate: cfg.sample_rate,
        })
    }

    pub(super) fn audio_frame_size(&self) -> usize {
        1024
    }

    pub(super) fn encode_video(&mut self, _frame: &frame::Video) -> Result<()> {
        Err(anyhow!(
            "desktop frames must be produced by the decode worker"
        ))
    }

    pub(super) fn encode_audio(&mut self, _frame: &frame::Audio) -> Result<()> {
        Err(anyhow!(
            "desktop audio must be produced by the decode worker"
        ))
    }

    pub(super) fn run_operation<T, F>(&mut self, benchmark: BenchHandle, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut DesktopFrameSender) -> T + Send + 'static,
    {
        let (video_sender, video_receiver) = sync_channel(VIDEO_CHANNEL_CAPACITY);
        let (audio_sender, audio_receiver) = sync_channel(AUDIO_CHANNEL_CAPACITY);
        let (control_sender, control_receiver) = sync_channel(CONTROL_CHANNEL_CAPACITY);
        let (discontinuity_sender, discontinuity_receiver) = sync_channel(1);
        let audio_effects = Arc::clone(&self.audio_effects);
        let audio_level_callback = self.audio_level_callback.clone();
        let audio_sample_rate = self.audio_sample_rate;
        let worker_benchmark = benchmark.clone();
        let worker = std_thread::Builder::new()
            .name("ffplayout-decode".to_string())
            .spawn(move || {
                benchmark::activate(worker_benchmark);
                let mut output = DesktopFrameSender {
                    video_sender,
                    audio_sender,
                    control_sender,
                    discontinuity_sender,
                    audio_effects,
                    audio_level_meter: AudioLevelMeter::new(
                        audio_sample_rate,
                        audio_level_callback,
                    ),
                    current_logo_opacity: 0.0,
                };
                let _ = output
                    .control_sender
                    .send(DesktopControlMessage::ClipStarted);
                let result = operation(&mut output);
                let _ = output
                    .control_sender
                    .send(DesktopControlMessage::ClipFinished);
                result
            })
            .map_err(|error| anyhow!("failed to start decode worker: {error}"))?;

        benchmark::activate(benchmark.clone());
        let render_result = self.renderer.run_clip(
            video_receiver,
            audio_receiver,
            control_receiver,
            discontinuity_receiver,
        );
        if let Err(error) = render_result {
            if error.downcast_ref::<PlaybackStopped>().is_some() {
                benchmark::detach();
                std_thread::spawn(move || {
                    if worker.join().is_err() {
                        log::warn!("decode worker panicked after desktop playback stopped");
                    }
                    benchmark::activate(benchmark);
                    benchmark::finish();
                });
                return Err(error);
            }

            let _ = worker.join();
            benchmark::finish();
            return Err(error);
        }

        let worker_result = worker
            .join()
            .map_err(|_| anyhow!("decode worker panicked"))?;
        benchmark::finish();
        Ok(worker_result)
    }

    pub(super) fn finish(self) -> Result<()> {
        self.renderer.finish()
    }
}

impl FrameOutput for DesktopFrameSender {
    fn audio_frame_size(&self) -> usize {
        1024
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        benchmark::measure(Stage::DesktopSend, || {
            self.video_sender.send(DesktopVideoMessage {
                frame: frame.clone(),
                logo_opacity: self.current_logo_opacity,
            })
        })
        .map_err(|_| PlaybackStopped.into())
    }

    fn apply_logo_overlay(
        &mut self,
        _frame: &mut frame::Video,
        _logo: &LogoOverlay,
        opacity_factor: f64,
    ) {
        self.current_logo_opacity = opacity_factor;
    }

    fn benchmarks_logo_overlay(&self) -> bool {
        false
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        if frame.samples() == 0 {
            return Ok(());
        }

        let (samples, samples_per_channel) = benchmark::measure(Stage::AudioProcess, || {
            let mut frame = frame.clone();
            self.audio_effects
                .lock()
                .map_err(|_| anyhow!("audio effect chain lock poisoned"))?
                .process(&mut frame);
            self.audio_level_meter.process_frame(&frame);
            let left = frame.plane::<f32>(0);
            let right = frame.plane::<f32>(1);
            let mut interleaved = Vec::with_capacity(frame.samples() * AUDIO_CHANNELS);
            for (left, right) in left.iter().zip(right) {
                interleaved.push(if left.is_finite() { *left } else { 0.0 });
                interleaved.push(if right.is_finite() { *right } else { 0.0 });
            }
            Ok::<_, anyhow::Error>((interleaved, frame.samples()))
        })?;

        benchmark::measure(Stage::DesktopSend, || {
            self.audio_sender
                .send(DesktopAudioMessage {
                    samples,
                    samples_per_channel,
                })
                .map_err(|_| PlaybackStopped.into())
        })
    }

    fn reset_after_skip(&mut self, video_pts: i64, audio_pts: i64) -> Result<bool> {
        self.discontinuity_sender
            .send(DesktopDiscontinuity {
                video_pts,
                audio_pts,
            })
            .map_err(|_| PlaybackStopped)?;
        Ok(true)
    }

    fn set_video_end(&mut self, video_end_pts: Option<i64>) -> Result<()> {
        self.control_sender
            .send(DesktopControlMessage::VideoEnd(video_end_pts))
            .map_err(|_| PlaybackStopped.into())
    }

    fn write_vtt_subtitles(
        &mut self,
        media_path: &str,
        output_start_ms: i64,
        source_start_ms: i64,
    ) -> Result<()> {
        let vtt_path = vtt::sidecar_path(media_path);
        if !vtt_path.exists() {
            return Ok(());
        }

        let subtitles = vtt::parse_file(&vtt_path)?
            .into_iter()
            .filter(|cue| cue.end_ms > source_start_ms)
            .map(|cue| DesktopSubtitleCue {
                start_ms: output_start_ms + cue.start_ms.saturating_sub(source_start_ms),
                end_ms: output_start_ms + cue.end_ms - source_start_ms,
                text: cue.text,
            })
            .collect();

        self.control_sender
            .send(DesktopControlMessage::Subtitles(subtitles))
            .map_err(|_| PlaybackStopped.into())
    }

    fn video_finished(&mut self) -> Result<()> {
        self.control_sender
            .send(DesktopControlMessage::VideoFinished)
            .map_err(|_| PlaybackStopped.into())
    }
}

impl DesktopRenderer {
    fn open(cfg: &OutputConfig) -> Result<Self> {
        prepare_desktop_window(cfg.width, cfg.height, cfg.desktop_fullscreen)?;
        let audio = DesktopAudio::open(cfg.sample_rate)?;
        let device_buffer_samples = audio.device_buffer_samples();
        let logo = cfg
            .logo
            .as_ref()
            .map(|logo| create_desktop_logo(logo, cfg.width, cfg.height))
            .transpose()?;

        Ok(Self {
            audio,
            audio_effects_control: cfg.audio_effects.clone(),
            video_queue: VecDeque::with_capacity(VIDEO_CHANNEL_CAPACITY),
            pending_audio: VecDeque::new(),
            pending_audio_samples: 0,
            submitted_audio_samples: 0,
            audio_started: false,
            fps: cfg.fps,
            sample_rate: cfg.sample_rate,
            device_buffer_samples,
            audio_clock: AudioMasterClock::new(cfg.sample_rate, device_buffer_samples),
            video_time_base: cfg.video_time_base,
            video_end_pts: None,
            video_finished: false,
            last_rendered_video_pts: None,
            last_video_present: None,
            last_starvation_report: None,
            subtitles_enabled: true,
            subtitles: Vec::new(),
            active_subtitle_text: None,
            subtitle_bitmap: None,
            logo,
            current_logo_opacity: 0.0,
            aspect_width: cfg.width.max(1),
            aspect_height: cfg.height.max(1),
            last_window_size: (cfg.width, cfg.height),
            pending_aspect_resize: None,
            volume_overlay_until: None,
            last_video: None,
            frame_converter: DesktopFrameConverter::default(),
        })
    }

    fn with_window<T>(&self, operation: impl FnOnce(&mut DesktopWindow) -> T) -> T {
        DESKTOP_WINDOW.with(|window| {
            let mut window = window.borrow_mut();
            operation(
                window
                    .as_mut()
                    .expect("desktop window must be initialized before rendering"),
            )
        })
    }

    fn run_clip(
        &mut self,
        video_receiver: Receiver<DesktopVideoMessage>,
        audio_receiver: Receiver<DesktopAudioMessage>,
        control_receiver: Receiver<DesktopControlMessage>,
        discontinuity_receiver: Receiver<DesktopDiscontinuity>,
    ) -> Result<()> {
        let mut clip_finished = false;
        loop {
            self.handle_events()?;
            self.apply_pending_window_aspect_constraint();
            if let Ok(discontinuity) = discontinuity_receiver.try_recv() {
                self.apply_discontinuity(discontinuity);
                return Ok(());
            }

            self.flush_pending_audio()?;
            let (mut received, control_drained) =
                self.drain_control_messages(&control_receiver, &mut clip_finished)?;
            let (audio_received, audio_drained) = self.drain_audio_messages(&audio_receiver);
            let (video_received, video_drained) = self.drain_video_messages(&video_receiver);
            received |= audio_received || video_received;

            if let Ok(discontinuity) = discontinuity_receiver.try_recv() {
                self.apply_discontinuity(discontinuity);
                return Ok(());
            }

            self.flush_pending_audio()?;
            self.render_due_video()?;

            // Control messages are sent after all media frames. With separate
            // channels, wait until both media queues were drained before
            // returning, preserving the ordering of the previous shared queue.
            if clip_finished && control_drained && audio_drained && video_drained {
                return Ok(());
            }

            if !received {
                std_thread::sleep(SCHEDULER_INTERVAL);
            }
        }
    }

    fn drain_control_messages(
        &mut self,
        receiver: &Receiver<DesktopControlMessage>,
        clip_finished: &mut bool,
    ) -> Result<(bool, bool)> {
        let mut received = false;
        loop {
            match receiver.try_recv() {
                Ok(DesktopControlMessage::ClipStarted) => {
                    received = true;
                    self.video_end_pts = None;
                    self.video_finished = false;
                    self.last_starvation_report = None;
                    self.subtitles.clear();
                    self.active_subtitle_text = None;
                    self.subtitle_bitmap = None;
                }
                Ok(DesktopControlMessage::Subtitles(subtitles)) => {
                    received = true;
                    self.subtitles = subtitles;
                    self.active_subtitle_text = None;
                    self.subtitle_bitmap = None;
                }
                Ok(DesktopControlMessage::VideoEnd(video_end_pts)) => {
                    received = true;
                    self.video_end_pts = video_end_pts;
                }
                Ok(DesktopControlMessage::VideoFinished) => {
                    received = true;
                    self.video_finished = true;
                    self.start_audio_if_ready(false);
                }
                Ok(DesktopControlMessage::ClipFinished) => {
                    received = true;
                    *clip_finished = true;
                }
                Err(TryRecvError::Empty) => return Ok((received, true)),
                Err(TryRecvError::Disconnected) if *clip_finished => return Ok((received, true)),
                Err(TryRecvError::Disconnected) => {
                    return Err(anyhow!("decode worker disconnected"));
                }
            }
        }
    }

    fn drain_audio_messages(&mut self, receiver: &Receiver<DesktopAudioMessage>) -> (bool, bool) {
        let mut received = false;
        while self.pending_audio_samples < self.max_pending_samples() {
            match receiver.try_recv() {
                Ok(DesktopAudioMessage {
                    samples,
                    samples_per_channel,
                }) => {
                    received = true;
                    self.pending_audio.push_back((samples, samples_per_channel));
                    self.pending_audio_samples = self
                        .pending_audio_samples
                        .saturating_add(samples_per_channel as u64);
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return (received, true),
            }
        }
        (received, false)
    }

    fn drain_video_messages(&mut self, receiver: &Receiver<DesktopVideoMessage>) -> (bool, bool) {
        let mut received = false;
        while self.video_queue.len() < VIDEO_CHANNEL_CAPACITY {
            match receiver.try_recv() {
                Ok(DesktopVideoMessage {
                    frame,
                    logo_opacity,
                }) => {
                    received = true;
                    self.current_logo_opacity = logo_opacity;
                    self.video_queue.push_back(frame);
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return (received, true),
            }
        }
        (received, false)
    }

    fn apply_discontinuity(&mut self, discontinuity: DesktopDiscontinuity) {
        let video_pts = discontinuity.video_pts.max(0);
        let audio_pts = discontinuity.audio_pts.max(0) as u64;

        self.audio.pause();
        self.audio.clear();
        self.video_queue.clear();
        self.pending_audio.clear();
        self.pending_audio_samples = 0;
        self.submitted_audio_samples = audio_pts;
        self.audio_started = false;
        self.audio_clock.reset_at(audio_pts, Instant::now());
        self.video_end_pts = None;
        self.video_finished = false;
        self.last_rendered_video_pts = video_pts.checked_sub(1);
        self.last_video_present = None;
        self.last_starvation_report = None;
        self.subtitles.clear();
        self.active_subtitle_text = None;
        self.subtitle_bitmap = None;
        self.last_video = None;
        self.refresh_window();
    }

    fn finish(mut self) -> Result<()> {
        self.flush_pending_audio()?;
        self.start_audio_if_ready(true);
        let remaining_samples = self.pending_audio_samples + self.queued_audio_samples();
        let deadline = Instant::now()
            + Duration::from_secs_f64(remaining_samples as f64 / f64::from(self.sample_rate))
            + Duration::from_secs(2);
        while !self.pending_audio.is_empty() || self.queued_audio_samples() > 0 {
            if Instant::now() >= deadline {
                log::warn!("desktop audio did not drain in time; finishing playback anyway");
                break;
            }
            self.handle_events()?;
            self.flush_pending_audio()?;
            self.render_due_video()?;
            std_thread::sleep(SCHEDULER_INTERVAL);
        }
        std_thread::sleep(Duration::from_secs_f64(
            self.device_buffer_samples as f64 / f64::from(self.sample_rate),
        ));
        self.render_due_video_at(self.submitted_audio_samples)
    }

    fn flush_pending_audio(&mut self) -> Result<()> {
        while self.queued_audio_samples() < self.max_queue_samples() {
            let Some((samples, samples_per_channel)) = self.pending_audio.pop_front() else {
                break;
            };
            self.audio.queue(&samples)?;
            self.pending_audio_samples = self
                .pending_audio_samples
                .saturating_sub(samples_per_channel as u64);
            self.submitted_audio_samples = self
                .submitted_audio_samples
                .saturating_add(samples_per_channel as u64);
        }
        self.start_audio_if_ready(false);
        Ok(())
    }

    fn start_audio_if_ready(&mut self, force: bool) {
        let video_ready = video_prebuffer_ready(self.video_queue.len(), self.video_finished, force);
        if !self.audio_started
            && video_ready
            && (force || self.queued_audio_samples() >= self.prebuffer_samples())
        {
            self.audio.resume();
            self.audio_started = true;
            let consumed_samples = self
                .submitted_audio_samples
                .saturating_sub(self.queued_audio_samples());
            self.audio_clock.reset_at(consumed_samples, Instant::now());
        }
    }

    fn render_due_video(&mut self) -> Result<()> {
        if !self.audio_started {
            return Ok(());
        }

        let audio_samples = self.played_audio_samples();
        self.render_due_video_at(audio_samples)
    }

    fn render_due_video_at(&mut self, audio_pts: u64) -> Result<()> {
        let mut dropped_frames = 0_u64;
        let expected_video_pts = self.expected_video_pts(audio_pts);

        while let Some(frame) = self.video_queue.front() {
            let frame_pts = frame.pts().unwrap_or_default().max(0);
            let already_rendered = self
                .last_rendered_video_pts
                .is_some_and(|last_pts| frame_pts <= last_pts);
            let too_late =
                video_frame_is_too_late(frame_pts, expected_video_pts, self.video_queue.len());
            if already_rendered || too_late {
                self.video_queue.pop_front();
                dropped_frames += 1;
                continue;
            }
            break;
        }

        if dropped_frames > 0 {
            log::trace!(
                "dropped {dropped_frames} late desktop video frame(s) at audio sample {audio_pts}"
            );
        }
        let video_time_base = self.video_time_base;
        let sample_rate = self.sample_rate;
        if let Some(frame) = self.video_queue.pop_front_if(|frame| {
            let frame_pts = frame.pts().unwrap_or_default().max(0);
            video_pts_in_audio_samples(frame_pts, video_time_base, sample_rate) <= audio_pts
        }) {
            self.render_video_frame(&frame)?;
            self.last_rendered_video_pts = frame.pts();
        }
        self.handle_video_starvation(audio_pts)
    }

    fn handle_video_starvation(&mut self, audio_pts: u64) -> Result<()> {
        let expected_video_pts = self.expected_video_pts(audio_pts);
        let last_video_pts = self.last_rendered_video_pts.unwrap_or_default();
        let next_video_pts = self
            .video_queue
            .front()
            .and_then(|frame| frame.pts())
            .unwrap_or(i64::MAX);
        let starved =
            expected_video_pts > last_video_pts + 1 && next_video_pts > expected_video_pts;
        let reached_video_end = self
            .video_end_pts
            .is_some_and(|video_end_pts| expected_video_pts >= video_end_pts);
        let now = Instant::now();
        let may_report = self
            .last_starvation_report
            .is_none_or(|last_report| now.duration_since(last_report) >= Duration::from_secs(1));

        if starved && !self.video_finished && !reached_video_end && may_report {
            log::debug!(
                "desktop video queue starved: expected pts {expected_video_pts}, last rendered \
                 pts {last_video_pts}, queued frames {}",
                self.video_queue.len()
            );
            self.last_starvation_report = Some(now);
        }

        if (self.video_finished || reached_video_end)
            && starved
            && expected_video_pts - last_video_pts > VIDEO_STARVATION_GRACE_FRAMES
        {
            self.last_video = None;
            self.refresh_window();
            self.last_rendered_video_pts = Some(expected_video_pts);
        }
        Ok(())
    }

    fn expected_video_pts(&self, audio_pts: u64) -> i64 {
        (audio_pts as i64)
            .rescale(Rational(1, self.sample_rate as i32), self.video_time_base)
            .max(0)
    }

    fn render_video_frame(&mut self, frame: &frame::Video) -> Result<()> {
        let now = Instant::now();
        if let Some(last_present) = self.last_video_present {
            let interval = now.duration_since(last_present);
            let frame_duration = Duration::from_secs_f64(f64::from(self.video_time_base));
            if interval < frame_duration.mul_f64(0.5) || interval > frame_duration.mul_f64(1.5) {
                log::trace!(
                    "desktop video presentation interval: {:.3} ms at pts {}",
                    interval.as_secs_f64() * 1_000.0,
                    frame.pts().unwrap_or_default()
                );
            }
        }
        self.last_video_present = Some(now);

        benchmark::measure_success(Stage::DesktopConvert, || {
            self.last_video = Some(self.frame_converter.convert(frame)?);
            self.refresh_window();
            Ok(())
        })
    }

    fn queued_audio_samples(&self) -> u64 {
        self.audio.queued_samples()
    }

    fn played_audio_samples(&mut self) -> u64 {
        self.audio_clock.position(
            self.submitted_audio_samples,
            self.queued_audio_samples(),
            Instant::now(),
        )
    }

    fn prebuffer_samples(&self) -> u64 {
        u64::from(self.sample_rate) * AUDIO_PREBUFFER_MS / 1_000
    }

    fn max_queue_samples(&self) -> u64 {
        u64::from(self.sample_rate) * AUDIO_MAX_QUEUE_MS / 1_000
    }

    fn max_pending_samples(&self) -> u64 {
        u64::from(self.sample_rate) * AUDIO_MAX_PENDING_MS / 1_000
    }

    fn handle_events(&mut self) -> Result<()> {
        self.with_window(DesktopWindow::pump_events);
        let actions = self.with_window(DesktopWindow::take_actions);
        let mut refresh = self
            .volume_overlay_until
            .is_some_and(|until| Instant::now() >= until);
        if refresh {
            self.volume_overlay_until = None;
        }

        for action in actions {
            match action {
                WindowAction::Stop => return Err(PlaybackStopped.into()),
                WindowAction::Resize(width, height) => {
                    if width > 0 && height > 0 {
                        // Fullscreen changes emit this after the toggle action.
                        // Rebuild using the actual physical window dimensions.
                        self.active_subtitle_text = None;
                        self.subtitle_bitmap = None;
                        refresh = true;
                    }
                    if !self.with_window(|window| window.fullscreen()) && width > 0 && height > 0 {
                        self.pending_aspect_resize = Some((width, height, Instant::now()));
                    }
                }
                WindowAction::ToggleSubtitles => {
                    self.subtitles_enabled = !self.subtitles_enabled;
                    self.active_subtitle_text = None;
                    self.subtitle_bitmap = None;
                    refresh = true;
                }
                WindowAction::AdjustVolume(delta) => {
                    let volume = adjusted_volume(self.audio_effects_control.volume(), delta);
                    self.audio_effects_control.set_volume(volume)?;
                    self.volume_overlay_until = Some(Instant::now() + VOLUME_OVERLAY_DURATION);
                    refresh = true;
                }
                WindowAction::FullscreenChanged => {
                    self.active_subtitle_text = None;
                    self.subtitle_bitmap = None;
                    refresh = true;
                }
            }
        }

        if refresh {
            self.refresh_window();
        }
        Ok(())
    }

    fn apply_pending_window_aspect_constraint(&mut self) {
        let Some((width, height, at)) = self.pending_aspect_resize else {
            return;
        };
        if Instant::now().duration_since(at) < WINDOW_ASPECT_SETTLE {
            return;
        }

        self.pending_aspect_resize = None;
        let (last_width, last_height) = self.last_window_size;
        let width_delta = width.abs_diff(last_width);
        let height_delta = height.abs_diff(last_height);
        let target = if width_delta >= height_delta {
            (
                width,
                scaled_aspect_dimension(width, self.aspect_height, self.aspect_width),
            )
        } else {
            (
                scaled_aspect_dimension(height, self.aspect_width, self.aspect_height),
                height,
            )
        };
        self.last_window_size = target;
        if target != (width, height) {
            self.with_window(|window| window.request_size(target.0, target.1));
        }
    }

    fn refresh_window(&mut self) {
        let subtitle = self.subtitle_for_current_frame();
        let logo = self.logo.as_ref().and_then(|logo| {
            let opacity = (f64::from(logo.opacity) * self.current_logo_opacity)
                .round()
                .clamp(0.0, 255.0) as u8;
            (opacity > 0).then(|| WindowLogo {
                bitmap: logo.bitmap.clone(),
                position: logo.position.clone(),
                opacity,
            })
        });
        let volume_overlay = self.volume_overlay_until.is_some();
        self.with_window(|window| {
            window.set_frame(WindowFrame {
                video: self.last_video.clone(),
                subtitle,
                logo,
                volume: self.audio_effects_control.volume(),
                volume_overlay,
            });
        });
    }

    fn subtitle_for_current_frame(&mut self) -> Option<RgbaBitmap> {
        let video_pts = self.last_video.as_ref()?.pts;
        let text = self.active_subtitle_for_pts(video_pts);
        if self.active_subtitle_text.as_deref() != text.as_deref() {
            self.active_subtitle_text = text.clone();
            self.subtitle_bitmap = text.and_then(|text| {
                self.with_window(|window| {
                    create_subtitle_bitmap(&text, window.size().0, window.uses_large_subtitles())
                })
                .map_err(|error| log::warn!("failed to render desktop subtitle: {error}"))
                .ok()
                .flatten()
            });
        }
        self.subtitle_bitmap.clone()
    }

    fn active_subtitle_for_pts(&self, video_pts: i64) -> Option<String> {
        if !self.subtitles_enabled {
            return None;
        }
        let ms = video_pts.saturating_mul(1_000) / i64::from(self.fps);
        self.subtitles
            .iter()
            .find(|cue| cue.start_ms <= ms && ms < cue.end_ms)
            .map(|cue| cue.text.clone())
    }
}

impl Drop for DesktopRenderer {
    fn drop(&mut self) {
        self.with_window(DesktopWindow::hide);
    }
}

impl Default for DesktopFrameConverter {
    fn default() -> Self {
        Self {
            scaler: None,
            converted: frame::Video::empty(),
        }
    }
}

impl DesktopFrameConverter {
    fn convert(&mut self, frame: &frame::Video) -> Result<VideoSurface> {
        let width = frame.width();
        let height = frame.height();
        if width == 0 || height == 0 {
            return Err(anyhow!("desktop video frame has zero dimensions"));
        }

        let reconfigure = self.scaler.as_ref().is_none_or(|scaler| {
            let input = scaler.input();
            input.format != frame.format() || input.width != width || input.height != height
        });
        if reconfigure {
            if let Some(scaler) = &mut self.scaler {
                scaler.cached(
                    frame.format(),
                    width,
                    height,
                    Pixel::BGRZ,
                    width,
                    height,
                    scaling::flag::Flags::FAST_BILINEAR,
                );
            } else {
                self.scaler = Some(scaling::Context::get(
                    frame.format(),
                    width,
                    height,
                    Pixel::BGRZ,
                    width,
                    height,
                    scaling::flag::Flags::FAST_BILINEAR,
                )?);
            }
            self.converted = frame::Video::empty();
        }
        self.scaler
            .as_mut()
            .expect("desktop scaler must be initialized")
            .run(frame, &mut self.converted)?;

        let mut pixels = vec![0_u32; width as usize * height as usize];
        let stride = self.converted.stride(0) / 4;
        for (target_row, source_row) in pixels
            .chunks_exact_mut(width as usize)
            .zip(self.converted.plane::<[u8; 4]>(0).chunks_exact(stride))
        {
            for (target, source) in target_row.iter_mut().zip(source_row) {
                *target = bgrz_to_softbuffer_pixel(*source);
            }
        }

        Ok(VideoSurface {
            width,
            height,
            pixels: pixels.into(),
            pts: frame.pts().unwrap_or_default(),
        })
    }
}

fn bgrz_to_softbuffer_pixel([blue, green, red, _]: [u8; 4]) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}

struct DesktopWindow {
    event_loop: EventLoop<()>,
    app: DesktopWindowApp,
}

struct DesktopWindowApp {
    window: Arc<Window>,
    renderer: SoftbufferRenderer,
    actions: Vec<WindowAction>,
    frame: Option<WindowFrame>,
    size: (u32, u32),
    occluded: bool,
}

fn prepare_desktop_window(width: u32, height: u32, fullscreen: bool) -> Result<()> {
    DESKTOP_WINDOW.with(|window| {
        let mut window = window.borrow_mut();
        if let Some(window) = window.as_mut() {
            window.reconfigure(width, height, fullscreen)
        } else {
            *window = Some(DesktopWindow::open(width, height, fullscreen)?);
            Ok(())
        }
    })
}

#[derive(Clone)]
struct WindowFrame {
    video: Option<VideoSurface>,
    subtitle: Option<RgbaBitmap>,
    logo: Option<WindowLogo>,
    volume: f64,
    volume_overlay: bool,
}

#[derive(Clone)]
struct WindowLogo {
    bitmap: RgbaBitmap,
    position: String,
    opacity: u8,
}

enum WindowAction {
    Stop,
    Resize(u32, u32),
    ToggleSubtitles,
    AdjustVolume(f64),
    FullscreenChanged,
}

impl DesktopWindow {
    fn open(width: u32, height: u32, fullscreen: bool) -> Result<Self> {
        let mut event_loop_builder = EventLoop::<()>::builder();
        // Desktop sessions are serialized on the persistent desktop worker.
        // Wayland and X11 share this Linux platform flag, which permits that
        // worker to own the event loop instead of the process main thread.
        #[cfg(target_os = "linux")]
        EventLoopBuilderExtWayland::with_any_thread(&mut event_loop_builder, true);
        let event_loop = event_loop_builder
            .build()
            .context("creating desktop window event loop")?;
        let attributes = Window::default_attributes()
            .with_title("ffplayout")
            .with_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
            .with_resizable(true)
            .with_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
        #[allow(deprecated)]
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("creating desktop window")?,
        );
        let size = window.inner_size();
        let mut renderer = SoftbufferRenderer::new(Arc::clone(&window))?;
        renderer.resize(size.width, size.height)?;

        Ok(Self {
            event_loop,
            app: DesktopWindowApp {
                window,
                renderer,
                actions: Vec::new(),
                frame: None,
                size: (size.width, size.height),
                occluded: false,
            },
        })
    }

    fn reconfigure(&mut self, width: u32, height: u32, fullscreen: bool) -> Result<()> {
        self.app.frame = None;
        self.app.actions.clear();
        self.app.occluded = false;
        self.app.window.set_visible(true);
        self.app.window.set_fullscreen(
            fullscreen.then(|| Fullscreen::Borderless(self.app.window.current_monitor())),
        );

        if !fullscreen && width > 0 && height > 0 {
            self.app.size = (width, height);
            self.app.renderer.resize(width, height)?;
            let _ = self
                .app
                .window
                .request_inner_size(PhysicalSize::new(width, height));
        }
        self.app.window.request_redraw();
        Ok(())
    }

    fn hide(&mut self) {
        self.app.frame = None;
        self.app.window.set_visible(false);
    }

    fn pump_events(&mut self) {
        let _ = self
            .event_loop
            .pump_app_events(Some(Duration::ZERO), &mut self.app);
    }

    fn take_actions(&mut self) -> Vec<WindowAction> {
        std::mem::take(&mut self.app.actions)
    }

    fn set_frame(&mut self, frame: WindowFrame) {
        self.app.frame = Some(frame);
        if !self.app.occluded {
            self.app.window.request_redraw();
        }
    }

    fn fullscreen(&self) -> bool {
        self.app.window.fullscreen().is_some()
    }

    fn uses_large_subtitles(&self) -> bool {
        self.fullscreen() || self.app.window.is_maximized()
    }

    fn size(&self) -> (u32, u32) {
        self.app.size
    }

    fn request_size(&self, width: u32, height: u32) {
        let _ = self
            .app
            .window
            .request_inner_size(PhysicalSize::new(width, height));
    }
}

impl ApplicationHandler for DesktopWindowApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => self.actions.push(WindowAction::Stop),
            WindowEvent::Resized(size) => {
                self.size = (size.width, size.height);
                if let Err(error) = self.renderer.resize(size.width, size.height) {
                    log::warn!("desktop renderer resize failed: {error}");
                }
                self.actions
                    .push(WindowAction::Resize(size.width, size.height));
                self.window.request_redraw();
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    self.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if !self.occluded
                    && let Some(frame) = &self.frame
                    && let Err(error) = benchmark::measure_success(Stage::DesktopPresent, || {
                        self.renderer.render(frame, self.size)
                    })
                {
                    log::warn!("desktop renderer failed: {error}");
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => self.actions.push(WindowAction::Stop),
                    PhysicalKey::Code(KeyCode::KeyF) if !event.repeat => {
                        let fullscreen = self.window.fullscreen().is_none();
                        self.window.set_fullscreen(
                            fullscreen
                                .then(|| Fullscreen::Borderless(self.window.current_monitor())),
                        );
                        self.actions.push(WindowAction::FullscreenChanged);
                    }
                    PhysicalKey::Code(KeyCode::KeyS) if !event.repeat => {
                        self.actions.push(WindowAction::ToggleSubtitles);
                    }
                    PhysicalKey::Code(KeyCode::ArrowLeft) => {
                        self.actions
                            .push(WindowAction::AdjustVolume(-DESKTOP_VOLUME_STEP));
                    }
                    PhysicalKey::Code(KeyCode::ArrowRight) => {
                        self.actions
                            .push(WindowAction::AdjustVolume(DESKTOP_VOLUME_STEP));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

struct SoftbufferRenderer {
    surface: Surface<Arc<Window>, Arc<Window>>,
    _context: SoftbufferContext<Arc<Window>>,
    width: u32,
    height: u32,
}

impl SoftbufferRenderer {
    fn new(window: Arc<Window>) -> Result<Self> {
        let context =
            SoftbufferContext::new(Arc::clone(&window)).map_err(|error| anyhow!("{error}"))?;
        let surface = Surface::new(&context, window).map_err(|error| anyhow!("{error}"))?;
        Ok(Self {
            surface,
            _context: context,
            width: 0,
            height: 0,
        })
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.width = width;
        self.height = height;
        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return Ok(());
        };
        self.surface
            .resize(width, height)
            .map_err(|error| anyhow!("{error}"))
    }

    fn render(&mut self, frame: &WindowFrame, size: (u32, u32)) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Ok(());
        }
        let mut target = self
            .surface
            .buffer_mut()
            .map_err(|error| anyhow!("{error}"))?;
        if let Some(video) = &frame.video {
            let rect = fit_rect(video.width, video.height, self.width, self.height);
            if rect.width != self.width || rect.height != self.height {
                target.fill(0);
            }
            scale_nearest(video, &mut target, self.width, rect);
        } else {
            target.fill(0);
        }
        if let Some(logo) = &frame.logo {
            draw_logo(&mut target, self.width, self.height, size, logo)?;
        }
        if let Some(subtitle) = &frame.subtitle {
            draw_subtitle(&mut target, self.width, self.height, subtitle);
        }
        if frame.volume_overlay {
            draw_volume_overlay(&mut target, self.width, self.height, frame.volume);
        }
        target.present().map_err(|error| anyhow!("{error}"))
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn fit_rect(src_width: u32, src_height: u32, dst_width: u32, dst_height: u32) -> Rect {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let scale = (dst_width as f64 / src_width as f64).min(dst_height as f64 / src_height as f64);
    let width = (src_width as f64 * scale).round().max(1.0) as u32;
    let height = (src_height as f64 * scale).round().max(1.0) as u32;
    Rect {
        x: (dst_width - width) / 2,
        y: (dst_height - height) / 2,
        width,
        height,
    }
}

fn scale_nearest(video: &VideoSurface, target: &mut [u32], stride: u32, dst: Rect) {
    if dst.width == 0 || dst.height == 0 {
        return;
    }
    if video.width == dst.width && video.height == dst.height {
        for row in 0..dst.height as usize {
            let source_start = row * video.width as usize;
            let target_start = (dst.y as usize + row) * stride as usize + dst.x as usize;
            target[target_start..target_start + dst.width as usize]
                .copy_from_slice(&video.pixels[source_start..source_start + video.width as usize]);
        }
        return;
    }
    for dy in 0..dst.height {
        let sy = (dy as u64 * video.height as u64 / dst.height as u64) as u32;
        for dx in 0..dst.width {
            let sx = (dx as u64 * video.width as u64 / dst.width as u64) as u32;
            let src = (sy * video.width + sx) as usize;
            let dest = ((dst.y + dy) * stride + dst.x + dx) as usize;
            target[dest] = video.pixels[src];
        }
    }
}

fn draw_logo(
    target: &mut [u32],
    width: u32,
    height: u32,
    window_size: (u32, u32),
    logo: &WindowLogo,
) -> Result<()> {
    let (x, y) = logo_position(
        &logo.position,
        window_size.0.max(1),
        window_size.1.max(1),
        logo.bitmap.width,
        logo.bitmap.height,
    )?;
    let scale_x = f64::from(width) / f64::from(window_size.0.max(1));
    let scale_y = f64::from(height) / f64::from(window_size.1.max(1));
    let x = (f64::from(x) * scale_x).round().max(0.0) as u32;
    let y = (f64::from(y) * scale_y).round().max(0.0) as u32;
    let scaled_width = (f64::from(logo.bitmap.width) * scale_x).round().max(1.0) as u32;
    let scaled_height = (f64::from(logo.bitmap.height) * scale_y).round().max(1.0) as u32;
    draw_bitmap_scaled(
        target,
        width,
        height,
        &logo.bitmap,
        Rect {
            x,
            y,
            width: scaled_width,
            height: scaled_height,
        },
        logo.opacity,
    );
    Ok(())
}

fn draw_subtitle(target: &mut [u32], width: u32, height: u32, subtitle: &RgbaBitmap) {
    let x = (width.saturating_sub(subtitle.width)) / 2;
    let y = height.saturating_sub(subtitle.height + SUBTITLE_MARGIN_BOTTOM);
    draw_bitmap_scaled(
        target,
        width,
        height,
        subtitle,
        Rect {
            x,
            y,
            width: subtitle.width,
            height: subtitle.height,
        },
        255,
    );
}

fn draw_bitmap_scaled(
    target: &mut [u32],
    target_width: u32,
    target_height: u32,
    bitmap: &RgbaBitmap,
    dst: Rect,
    opacity: u8,
) {
    if dst.width == 0 || dst.height == 0 {
        return;
    }
    let x_end = dst.x.saturating_add(dst.width).min(target_width);
    let y_end = dst.y.saturating_add(dst.height).min(target_height);
    for y in dst.y..y_end {
        let source_y = ((y - dst.y) as u64 * bitmap.height as u64 / dst.height as u64) as u32;
        for x in dst.x..x_end {
            let source_x = ((x - dst.x) as u64 * bitmap.width as u64 / dst.width as u64) as u32;
            let index = (source_y * bitmap.width + source_x) as usize * 4;
            let alpha = (u16::from(bitmap.pixels[index + 3]) * u16::from(opacity) / 255) as u8;
            if alpha > 0 {
                let color = (u32::from(bitmap.pixels[index]) << 16)
                    | (u32::from(bitmap.pixels[index + 1]) << 8)
                    | u32::from(bitmap.pixels[index + 2]);
                let target_index = (y * target_width + x) as usize;
                target[target_index] = blend(target[target_index], color, alpha);
            }
        }
    }
}

fn draw_volume_overlay(target: &mut [u32], width: u32, height: u32, volume: f64) {
    let panel_width = (width / 3).clamp(180, 360);
    let panel_height = 28;
    let margin = 24;
    let x = (width.saturating_sub(panel_width)) / 2;
    let y = height.saturating_sub(panel_height + margin);
    fill_blended_rect(
        target,
        width,
        height,
        Rect {
            x,
            y,
            width: panel_width,
            height: panel_height,
        },
        0x101214,
        220,
    );
    let track_margin = 8;
    let track_height = 6;
    let track_width = panel_width.saturating_sub(track_margin * 2);
    let track = Rect {
        x: x + track_margin,
        y: y + (panel_height - track_height) / 2,
        width: track_width,
        height: track_height,
    };
    fill_blended_rect(target, width, height, track, 0x4E5660, 255);
    let fill_width =
        ((volume / DESKTOP_VOLUME_MAX).clamp(0.0, 1.0) * f64::from(track_width)).round() as u32;
    fill_blended_rect(
        target,
        width,
        height,
        Rect {
            width: fill_width,
            ..track
        },
        0xEEEEEE,
        255,
    );
}

fn fill_blended_rect(
    target: &mut [u32],
    stride: u32,
    canvas_height: u32,
    rect: Rect,
    color: u32,
    alpha: u8,
) {
    let x_end = rect.x.saturating_add(rect.width).min(stride);
    let y_end = rect.y.saturating_add(rect.height).min(canvas_height);
    for y in rect.y..y_end {
        for x in rect.x..x_end {
            let index = (y * stride + x) as usize;
            target[index] = blend(target[index], color, alpha);
        }
    }
}

fn blend(dst: u32, src: u32, alpha: u8) -> u32 {
    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    let channel = |shift| {
        (((src >> shift) & 0xff_u32) * alpha + ((dst >> shift) & 0xff_u32) * inverse) / 255_u32
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn create_desktop_logo(
    config: &crate::utils::config::LogoConfig,
    output_width: u32,
    output_height: u32,
) -> Result<DesktopLogo> {
    let logo = LogoOverlay::load(config, output_width, output_height)?;
    Ok(DesktopLogo {
        bitmap: RgbaBitmap {
            pixels: yuva420p_to_rgba(&logo),
            width: logo.width,
            height: logo.height,
        },
        position: config.position.clone(),
        opacity: logo.opacity,
    })
}

fn yuva420p_to_rgba(logo: &LogoOverlay) -> Vec<u8> {
    let width = logo.width as usize;
    let height = logo.height as usize;
    let y_plane = logo.frame.data(0);
    let u_plane = logo.frame.data(1);
    let v_plane = logo.frame.data(2);
    let a_plane = logo.frame.data(3);
    let y_stride = logo.frame.stride(0);
    let u_stride = logo.frame.stride(1);
    let v_stride = logo.frame.stride(2);
    let a_stride = logo.frame.stride(3);
    let mut pixels = vec![0_u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = yuv_to_rgb(
                i32::from(y_plane[y * y_stride + x]),
                i32::from(u_plane[(y / 2) * u_stride + x / 2]),
                i32::from(v_plane[(y / 2) * v_stride + x / 2]),
            );
            let destination = (y * width + x) * 4;
            pixels[destination..destination + 4].copy_from_slice(&[
                r,
                g,
                b,
                a_plane[y * a_stride + x],
            ]);
        }
    }
    pixels
}

fn create_subtitle_bitmap(
    text: &str,
    window_width: u32,
    large_display: bool,
) -> Result<Option<RgbaBitmap>> {
    let max_width = (window_width * SUBTITLE_MAX_WIDTH_PERCENT / 100).max(1);
    let font_size = subtitle_font_size(large_display);
    let text_width = max_width.saturating_sub(SUBTITLE_OUTLINE * 2).max(1);
    let white = render_wrapped_text_bitmap(
        text,
        font_size,
        Weight::SEMIBOLD,
        RgbaColor {
            r: 248,
            g: 248,
            b: 248,
            a: 255,
        },
        text_width,
    )?;
    let black = render_wrapped_text_bitmap(
        text,
        font_size,
        Weight::SEMIBOLD,
        RgbaColor {
            r: 0,
            g: 0,
            b: 0,
            a: 230,
        },
        text_width,
    )?;
    let width = white.width + SUBTITLE_OUTLINE * 2;
    let height = white.height + SUBTITLE_OUTLINE * 2;
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    let outline = SUBTITLE_OUTLINE as i32;
    for y in -outline..=outline {
        for x in -outline..=outline {
            if x == 0 && y == 0 {
                continue;
            }
            let distance = ((x * x + y * y) as f32).sqrt();
            if distance <= outline as f32 + 0.25 {
                let alpha = (1.0 - distance / (outline as f32 + 0.75)).clamp(0.25, 1.0);
                composite_bitmap(&mut pixels, width, &black, outline + x, outline + y, alpha);
            }
        }
    }
    composite_bitmap(&mut pixels, width, &white, outline, outline, 1.0);
    Ok(Some(RgbaBitmap {
        pixels,
        width,
        height,
    }))
}

fn subtitle_font_size(large_display: bool) -> f32 {
    if large_display {
        SUBTITLE_FULLSCREEN_FONT_SIZE
    } else {
        SUBTITLE_FONT_SIZE
    }
}

fn composite_bitmap(
    destination: &mut [u8],
    destination_width: u32,
    source: &TextBitmap,
    x: i32,
    y: i32,
    alpha_scale: f32,
) {
    for source_y in 0..source.height as usize {
        let destination_y = y + source_y as i32;
        if destination_y < 0 {
            continue;
        }
        for source_x in 0..source.width as usize {
            let destination_x = x + source_x as i32;
            if destination_x < 0 {
                continue;
            }
            let destination_index =
                (destination_y as usize * destination_width as usize + destination_x as usize) * 4;
            let source_index = (source_y * source.width as usize + source_x) * 4;
            if destination_index + 4 > destination.len() {
                continue;
            }
            let pixel = [
                source.pixels[source_index],
                source.pixels[source_index + 1],
                source.pixels[source_index + 2],
                (f32::from(source.pixels[source_index + 3]) * alpha_scale).round() as u8,
            ];
            alpha_composite_rgba(
                &mut destination[destination_index..destination_index + 4],
                &pixel,
            );
        }
    }
}

fn alpha_composite_rgba(destination: &mut [u8], source: &[u8]) {
    let alpha = u16::from(source[3]);
    let inverse = 255 - alpha;
    for channel in 0..3 {
        destination[channel] =
            ((u16::from(destination[channel]) * inverse + u16::from(source[channel]) * alpha + 127)
                / 255) as u8;
    }
    destination[3] = (u16::from(destination[3]) + alpha
        - ((u16::from(destination[3]) * alpha + 127) / 255)) as u8;
}

fn yuv_to_rgb(y: i32, u: i32, v: i32) -> (u8, u8, u8) {
    let c = y.saturating_sub(16);
    let d = u - 128;
    let e = v - 128;
    (
        clamp_rgb((298 * c + 409 * e + 128) >> 8),
        clamp_rgb((298 * c - 100 * d - 208 * e + 128) >> 8),
        clamp_rgb((298 * c + 516 * d + 128) >> 8),
    )
}

fn clamp_rgb(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

struct AudioMasterClock {
    sample_rate: u32,
    device_buffer_samples: u64,
    last_consumed_samples: u64,
    anchor_samples: u64,
    anchor_time: Instant,
}

impl AudioMasterClock {
    fn new(sample_rate: u32, device_buffer_samples: u64) -> Self {
        Self {
            sample_rate,
            device_buffer_samples,
            last_consumed_samples: 0,
            anchor_samples: 0,
            anchor_time: Instant::now(),
        }
    }

    fn reset_at(&mut self, samples: u64, now: Instant) {
        self.last_consumed_samples = samples;
        self.anchor_samples = samples;
        self.anchor_time = now;
    }

    fn position(&mut self, submitted: u64, queued: u64, now: Instant) -> u64 {
        let consumed = submitted.saturating_sub(queued);
        if consumed != self.last_consumed_samples {
            self.last_consumed_samples = consumed;
            self.anchor_samples = consumed.saturating_sub(self.device_buffer_samples);
            self.anchor_time = now;
        }
        let elapsed_samples = (now.duration_since(self.anchor_time).as_secs_f64()
            * f64::from(self.sample_rate)) as u64;
        self.anchor_samples
            .saturating_add(elapsed_samples)
            .min(consumed)
    }
}

fn video_pts_in_audio_samples(video_pts: i64, video_time_base: Rational, sample_rate: u32) -> u64 {
    video_pts
        .rescale(video_time_base, Rational(1, sample_rate as i32))
        .max(0) as u64
}

fn adjusted_volume(volume: f64, delta: f64) -> f64 {
    (volume + delta).clamp(DESKTOP_VOLUME_MIN, DESKTOP_VOLUME_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_clock_interpolates_between_device_buffer_requests() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset_at(0, start);
        assert_eq!(clock.position(4_800, 3_776, start), 0);
        assert_eq!(
            clock.position(4_800, 3_776, start + Duration::from_millis(10)),
            480
        );
        assert_eq!(
            clock.position(4_800, 3_776, start + Duration::from_millis(30)),
            1_024
        );
    }

    #[test]
    fn audio_clock_reanchors_when_device_requests_another_buffer() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset_at(0, start);
        assert_eq!(clock.position(4_800, 2_752, start), 1_024);
        assert_eq!(
            clock.position(4_800, 2_752, start + Duration::from_millis(10)),
            1_504
        );
    }

    #[test]
    fn audio_clock_reanchors_at_skip_position() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset_at(96_000, start);
        assert_eq!(clock.position(100_800, 4_800, start), 96_000);
        assert_eq!(
            clock.position(100_800, 4_800, start + Duration::from_millis(10)),
            96_000
        );
        assert_eq!(
            clock.position(100_800, 3_776, start + Duration::from_millis(20)),
            96_000
        );
        assert_eq!(
            clock.position(100_800, 3_776, start + Duration::from_millis(30)),
            96_480
        );
    }

    #[test]
    fn video_pts_are_converted_to_audio_clock_samples() {
        assert_eq!(
            video_pts_in_audio_samples(25, Rational(1, 25), 48_000),
            48_000
        );
        assert_eq!(
            video_pts_in_audio_samples(1, Rational(1, 25), 48_000),
            1_920
        );
    }

    #[test]
    fn bgrz_pixels_match_softbuffer_rgb_layout() {
        assert_eq!(bgrz_to_softbuffer_pixel([0x33, 0x22, 0x11, 0]), 0x0011_2233);
    }

    #[test]
    fn maximized_subtitles_use_the_larger_font() {
        assert_eq!(subtitle_font_size(false), SUBTITLE_FONT_SIZE);
        assert_eq!(subtitle_font_size(true), SUBTITLE_FULLSCREEN_FONT_SIZE);
    }

    #[test]
    fn waits_for_video_prebuffer_before_starting_audio() {
        assert!(!video_prebuffer_ready(
            VIDEO_PREBUFFER_FRAMES - 1,
            false,
            false
        ));
        assert!(video_prebuffer_ready(VIDEO_PREBUFFER_FRAMES, false, false));
        assert!(video_prebuffer_ready(0, true, false));
    }

    #[test]
    fn desktop_audio_channel_has_more_headroom_than_video_channel() {
        const { assert!(AUDIO_CHANNEL_CAPACITY > VIDEO_CHANNEL_CAPACITY) };
    }

    #[test]
    fn only_drops_frames_that_are_severely_late() {
        assert!(!video_frame_is_too_late(10, 13, 2));
        assert!(video_frame_is_too_late(10, 14, 2));
        assert!(!video_frame_is_too_late(10, 20, 1));
    }

    #[test]
    fn desktop_volume_adjustment_is_clamped() {
        assert_eq!(adjusted_volume(1.0, 0.05), 1.05);
        assert_eq!(adjusted_volume(1.49, 0.05), DESKTOP_VOLUME_MAX);
        assert_eq!(adjusted_volume(0.01, -0.05), DESKTOP_VOLUME_MIN);
    }

    #[test]
    fn fit_rect_preserves_video_aspect_ratio() {
        let rect = fit_rect(1_920, 1_080, 1_280, 1_024);
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0, 152, 1_280, 720)
        );
    }

    #[test]
    fn one_to_one_video_copy_preserves_pixels() {
        let video = VideoSurface {
            width: 2,
            height: 2,
            pixels: Arc::from([1, 2, 3, 4]),
            pts: 0,
        };
        let mut target = [0; 4];

        scale_nearest(
            &video,
            &mut target,
            2,
            Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
        );

        assert_eq!(target, [1, 2, 3, 4]);
    }
}
