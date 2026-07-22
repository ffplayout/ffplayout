use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread as std_thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{Rational, Rescale, frame};
#[cfg(target_os = "linux")]
use winit::platform::{
    wayland::{EventLoopBuilderExtWayland, WindowAttributesExtWayland},
    x11::WindowAttributesExtX11,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    platform::pump_events::EventLoopExtPumpEvents,
    window::{Fullscreen, Icon, Window, WindowId},
};

use super::{FrameOutput, PlaybackStopped, vtt};
use crate::{
    analysis::audio_level::{AudioLevelCallback, AudioLevelMeter},
    audio_mixer::{AudioEffectChain, AudioEffectsControl},
    benchmark::{self, BenchHandle, Stage},
    compositor::logo::LogoOverlay,
    utils::config::{DesktopControlCallback, DesktopControlCommand, OutputConfig},
};

mod audio;
#[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
mod cpu;
#[cfg(feature = "desktop-gpu")]
mod gpu;
mod graphics;
mod icon;
mod render;
pub(crate) mod thread;
mod timing;
mod video;

use audio::DesktopAudio;
#[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
use cpu::WindowRenderer;
#[cfg(feature = "desktop-gpu")]
use gpu::WindowRenderer;
use graphics::{
    DesktopLogo, RgbaBitmap, create_desktop_logo, create_help_bitmap, create_subtitle_bitmap,
};
#[cfg(test)]
use graphics::{SUBTITLE_FONT_SIZE, SUBTITLE_FULLSCREEN_FONT_SIZE, subtitle_font_size};
use render::{WindowFrame, WindowLogo};
use timing::{AudioMasterClock, adjusted_volume, video_pts_in_audio_samples};
use video::{DesktopFrameConverter, VideoSurface};

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
const DESKTOP_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

const DESKTOP_WINDOW_TITLE: &str = "ffplayout";
const DESKTOP_APPLICATION_ID: &str = "ffplayout";

fn desktop_window_icon() -> Result<Icon> {
    Icon::from_rgba(
        icon::desktop_icon_rgba(),
        icon::DESKTOP_ICON_WIDTH,
        icon::DESKTOP_ICON_HEIGHT,
    )
    .context("creating desktop window icon")
}
const CONTROL_CHANNEL_CAPACITY: usize = 8;
const DESKTOP_VOLUME_STEP: f64 = 0.05;
const DESKTOP_VOLUME_MIN: f64 = 0.0;
const DESKTOP_VOLUME_MAX: f64 = 1.5;
const VOLUME_OVERLAY_DURATION: Duration = Duration::from_millis(900);
const WINDOW_ASPECT_SETTLE: Duration = Duration::from_millis(120);

fn video_prebuffer_ready(queue_len: usize, video_decoded: bool, force: bool) -> bool {
    force || video_decoded || queue_len >= VIDEO_PREBUFFER_FRAMES
}

fn scaled_aspect_dimension(value: u32, numerator: u32, denominator: u32) -> u32 {
    ((u64::from(value) * u64::from(numerator) + (u64::from(denominator) / 2))
        / u64::from(denominator))
    .max(1) as u32
}

fn video_frame_is_too_late(frame_pts: i64, expected_pts: i64, queue_len: usize) -> bool {
    queue_len > 1 && expected_pts.saturating_sub(frame_pts) > VIDEO_DROP_THRESHOLD_FRAMES
}

fn split_audio_padding(submitted: u64, virtual_position: u64, padding: u64) -> (u64, u64) {
    let covered = virtual_position.saturating_sub(submitted).min(padding);
    (covered, padding - covered)
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
    VideoDecoded,
    VideoFinished,
    AudioPadding(u64),
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
    pending_silence_samples: u64,
    submitted_audio_samples: u64,
    audio_started: bool,
    sample_rate: u32,
    device_buffer_samples: u64,
    audio_clock: AudioMasterClock,
    video_time_base: Rational,
    video_end_pts: Option<i64>,
    video_decoded: bool,
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
    desktop_control_callback: Option<DesktopControlCallback>,
    help_visible: bool,
    help_bitmap: Option<RgbaBitmap>,
}

thread_local! {
    static DESKTOP_WINDOW: RefCell<Option<DesktopWindow>> = const { RefCell::new(None) };
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

    fn video_decoded(&mut self) -> Result<()> {
        self.control_sender
            .send(DesktopControlMessage::VideoDecoded)
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

    fn pad_audio(&mut self, samples: i64) -> Result<bool> {
        let samples = u64::try_from(samples).map_err(|_| anyhow!("negative audio padding"))?;
        self.control_sender
            .send(DesktopControlMessage::AudioPadding(samples))
            .map_err(|_| PlaybackStopped)?;
        Ok(true)
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
            pending_silence_samples: 0,
            submitted_audio_samples: 0,
            audio_started: false,
            fps: cfg.fps,
            sample_rate: cfg.sample_rate,
            device_buffer_samples,
            audio_clock: AudioMasterClock::new(cfg.sample_rate, device_buffer_samples),
            video_time_base: cfg.video_time_base,
            video_end_pts: None,
            video_decoded: false,
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
            desktop_control_callback: cfg.desktop_control_callback.clone(),
            help_visible: false,
            help_bitmap: None,
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
                    self.video_decoded = false;
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
                Ok(DesktopControlMessage::VideoDecoded) => {
                    received = true;
                    self.video_decoded = true;
                    self.start_audio_if_ready(false);
                }
                Ok(DesktopControlMessage::VideoFinished) => {
                    received = true;
                    self.video_finished = true;
                    self.start_audio_if_ready(false);
                }
                Ok(DesktopControlMessage::AudioPadding(samples)) => {
                    received = true;
                    self.apply_audio_padding(samples);
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
        self.pending_silence_samples = 0;
        self.submitted_audio_samples = audio_pts;
        self.audio_started = false;
        self.audio_clock.reset_at(audio_pts, Instant::now());
        self.video_end_pts = None;
        self.video_decoded = false;
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
        let result = (|| {
            self.flush_pending_audio()?;
            self.start_audio_if_ready(true);
            let remaining_samples = self.pending_audio_samples
                + self.pending_silence_samples
                + self.queued_audio_samples();
            let deadline = Instant::now()
                + Duration::from_secs_f64(remaining_samples as f64 / f64::from(self.sample_rate))
                + Duration::from_secs(2);
            while !self.pending_audio.is_empty()
                || self.pending_silence_samples > 0
                || self.queued_audio_samples() > 0
            {
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
        })();
        drop(self);
        close_desktop_window();
        result
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
        while self.pending_audio.is_empty()
            && self.pending_silence_samples > 0
            && self.queued_audio_samples() < self.max_queue_samples()
        {
            let samples = self.pending_silence_samples.min(1_024) as usize;
            self.audio.queue(&vec![0.0; samples * AUDIO_CHANNELS])?;
            self.pending_silence_samples -= samples as u64;
            self.submitted_audio_samples =
                self.submitted_audio_samples.saturating_add(samples as u64);
        }
        self.start_audio_if_ready(false);
        Ok(())
    }

    fn apply_audio_padding(&mut self, samples: u64) {
        let queued = self.queued_audio_samples();
        let virtual_position = self.played_audio_samples();
        let (covered, remaining) =
            split_audio_padding(self.submitted_audio_samples, virtual_position, samples);
        self.submitted_audio_samples = self.submitted_audio_samples.saturating_add(covered);
        self.pending_silence_samples = self.pending_silence_samples.saturating_add(remaining);
        if covered > 0 {
            self.audio_clock.reset_at(
                self.submitted_audio_samples.saturating_sub(queued),
                Instant::now(),
            );
        }
    }

    fn start_audio_if_ready(&mut self, force: bool) {
        let video_ready = video_prebuffer_ready(self.video_queue.len(), self.video_decoded, force);
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
        let queued = self.queued_audio_samples();
        let allow_underflow = self.audio_started
            && queued == 0
            && self.pending_audio_samples == 0
            && self.video_queue.len() == VIDEO_CHANNEL_CAPACITY;
        self.audio_clock.position(
            self.submitted_audio_samples,
            queued,
            Instant::now(),
            allow_underflow,
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
                        self.help_bitmap = None;
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
                WindowAction::Control(command) => {
                    if let Some(callback) = &self.desktop_control_callback {
                        callback.invoke(command);
                    }
                }
                WindowAction::ToggleHelp => {
                    self.help_visible = !self.help_visible;
                    refresh = true;
                }
                WindowAction::AdjustVolume(delta) => {
                    let volume = adjusted_volume(
                        self.audio_effects_control.volume(),
                        delta,
                        DESKTOP_VOLUME_MIN,
                        DESKTOP_VOLUME_MAX,
                    );
                    self.audio_effects_control.set_volume(volume)?;
                    self.volume_overlay_until = Some(Instant::now() + VOLUME_OVERLAY_DURATION);
                    refresh = true;
                }
                WindowAction::FullscreenChanged => {
                    self.active_subtitle_text = None;
                    self.subtitle_bitmap = None;
                    self.help_bitmap = None;
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
        let help = self.help_bitmap();
        self.with_window(|window| {
            window.set_frame(WindowFrame {
                video: self.last_video.clone(),
                subtitle,
                logo,
                volume: self.audio_effects_control.volume(),
                volume_overlay,
                help,
            });
        });
    }

    fn help_bitmap(&mut self) -> Option<RgbaBitmap> {
        if !self.help_visible {
            return None;
        }
        if self.help_bitmap.is_none() {
            self.help_bitmap = self
                .with_window(|window| {
                    create_help_bitmap(window.size().0, window.uses_large_subtitles())
                })
                .map_err(|error| log::warn!("failed to render desktop help: {error}"))
                .ok()
                .flatten();
        }
        self.help_bitmap.clone()
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

struct DesktopWindow {
    event_loop: EventLoop<()>,
    app: DesktopWindowApp,
}

struct DesktopWindowApp {
    window: Arc<Window>,
    renderer: WindowRenderer,
    actions: Vec<WindowAction>,
    frame: Option<WindowFrame>,
    size: (u32, u32),
    occluded: bool,
    last_primary_click: Option<Instant>,
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

fn close_desktop_window() {
    DESKTOP_WINDOW.with(|window| {
        let window = window.borrow_mut().take();
        drop(window);
    });
}

enum WindowAction {
    Stop,
    Resize(u32, u32),
    ToggleSubtitles,
    AdjustVolume(f64),
    FullscreenChanged,
    Control(DesktopControlCommand),
    ToggleHelp,
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
            .with_title(DESKTOP_WINDOW_TITLE)
            .with_window_icon(Some(desktop_window_icon()?))
            .with_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
            .with_resizable(true)
            .with_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
        // Wayland uses this as the application ID; X11 uses it as the window class.
        // Both let desktop shells associate this window with ffplayout instead of "Unknown".
        #[cfg(target_os = "linux")]
        let attributes = WindowAttributesExtWayland::with_name(
            attributes,
            DESKTOP_APPLICATION_ID,
            DESKTOP_APPLICATION_ID,
        );
        #[cfg(target_os = "linux")]
        let attributes = WindowAttributesExtX11::with_name(
            attributes,
            DESKTOP_APPLICATION_ID,
            DESKTOP_APPLICATION_ID,
        );
        #[allow(deprecated)]
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("creating desktop window")?,
        );
        let size = window.inner_size();
        let renderer = WindowRenderer::new(
            Arc::clone(&window),
            event_loop.owned_display_handle(),
            width,
            height,
        )?;

        Ok(Self {
            event_loop,
            app: DesktopWindowApp {
                window,
                renderer,
                actions: Vec::new(),
                frame: None,
                size: (size.width, size.height),
                occluded: false,
                last_primary_click: None,
            },
        })
    }

    fn reconfigure(&mut self, width: u32, height: u32, fullscreen: bool) -> Result<()> {
        self.app.frame = None;
        self.app.actions.clear();
        self.app.occluded = false;
        self.app.renderer.reset_frame_cache();
        self.app.window.set_visible(true);
        self.app.window.set_fullscreen(
            fullscreen.then(|| Fullscreen::Borderless(self.app.window.current_monitor())),
        );

        if width > 0 && height > 0 {
            self.app.renderer.resize_buffer(width, height)?;
        }

        if !fullscreen && width > 0 && height > 0 {
            self.app.size = (width, height);
            self.app.renderer.resize_surface(width, height)?;
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
        self.app.renderer.release_frame_resources();
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

impl DesktopWindowApp {
    fn toggle_fullscreen(&mut self) {
        let fullscreen = self.window.fullscreen().is_none();
        self.window.set_fullscreen(
            fullscreen.then(|| Fullscreen::Borderless(self.window.current_monitor())),
        );
        self.actions.push(WindowAction::FullscreenChanged);
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
                if let Err(error) = self.renderer.resize_surface(size.width, size.height) {
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
                        self.toggle_fullscreen();
                    }
                    PhysicalKey::Code(KeyCode::KeyS) if !event.repeat => {
                        self.actions.push(WindowAction::ToggleSubtitles);
                    }
                    PhysicalKey::Code(KeyCode::KeyE) if !event.repeat => {
                        self.actions
                            .push(WindowAction::Control(DesktopControlCommand::Back));
                    }
                    PhysicalKey::Code(KeyCode::KeyT) if !event.repeat => {
                        self.actions
                            .push(WindowAction::Control(DesktopControlCommand::Next));
                    }
                    PhysicalKey::Code(KeyCode::KeyR) if !event.repeat => {
                        self.actions
                            .push(WindowAction::Control(DesktopControlCommand::Reset));
                    }
                    PhysicalKey::Code(KeyCode::KeyH) if !event.repeat => {
                        self.actions.push(WindowAction::ToggleHelp);
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
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let now = Instant::now();
                if self.last_primary_click.is_some_and(|last_click| {
                    now.duration_since(last_click) <= DESKTOP_DOUBLE_CLICK_INTERVAL
                }) {
                    self.last_primary_click = None;
                    self.toggle_fullscreen();
                } else {
                    self.last_primary_click = Some(now);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "desktop-gpu")]
    use super::gpu::yuv_color_parameters;
    use super::render::fit_rect;
    use super::*;
    #[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
    use super::{cpu::scale_nearest, render::Rect, video::bgrz_to_rgb_pixel};
    #[cfg(feature = "desktop-gpu")]
    use ffmpeg_next::util::color;

    #[test]
    fn embedded_desktop_icon_is_valid() {
        assert!(desktop_window_icon().is_ok());
    }

    #[test]
    fn audio_clock_interpolates_between_device_buffer_requests() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset_at(0, start);
        assert_eq!(clock.position(4_800, 3_776, start, false), 0);
        assert_eq!(
            clock.position(4_800, 3_776, start + Duration::from_millis(10), false),
            480
        );
        assert_eq!(
            clock.position(4_800, 3_776, start + Duration::from_millis(30), false),
            1_024
        );
    }

    #[test]
    fn audio_clock_reanchors_when_device_requests_another_buffer() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset_at(0, start);
        assert_eq!(clock.position(4_800, 2_752, start, false), 1_024);
        assert_eq!(
            clock.position(4_800, 2_752, start + Duration::from_millis(10), false),
            1_504
        );
    }

    #[test]
    fn audio_clock_reanchors_at_skip_position() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset_at(96_000, start);
        assert_eq!(clock.position(100_800, 4_800, start, false), 96_000);
        assert_eq!(
            clock.position(100_800, 4_800, start + Duration::from_millis(10), false),
            96_000
        );
        assert_eq!(
            clock.position(100_800, 3_776, start + Duration::from_millis(20), false),
            96_000
        );
        assert_eq!(
            clock.position(100_800, 3_776, start + Duration::from_millis(30), false),
            96_480
        );
    }

    #[test]
    fn audio_clock_advances_during_confirmed_underflow() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset_at(48_000, start);

        assert_eq!(
            clock.position(48_000, 0, start + Duration::from_secs(1), true),
            96_000
        );
    }

    #[test]
    fn audio_padding_only_queues_silence_not_covered_by_underflow() {
        assert_eq!(
            split_audio_padding(48_000, 72_000, 48_000),
            (24_000, 24_000)
        );
        assert_eq!(split_audio_padding(48_000, 48_000, 48_000), (0, 48_000));
        assert_eq!(split_audio_padding(48_000, 120_000, 48_000), (48_000, 0));
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

    #[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
    #[test]
    fn bgrz_pixels_are_converted_to_rgb() {
        assert_eq!(bgrz_to_rgb_pixel([0x33, 0x22, 0x11, 0]), 0x0011_2233);
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
        assert_eq!(
            adjusted_volume(1.0, 0.05, DESKTOP_VOLUME_MIN, DESKTOP_VOLUME_MAX),
            1.05
        );
        assert_eq!(
            adjusted_volume(1.49, 0.05, DESKTOP_VOLUME_MIN, DESKTOP_VOLUME_MAX),
            DESKTOP_VOLUME_MAX
        );
        assert_eq!(
            adjusted_volume(0.01, -0.05, DESKTOP_VOLUME_MIN, DESKTOP_VOLUME_MAX),
            DESKTOP_VOLUME_MIN
        );
    }

    #[test]
    fn fit_rect_preserves_video_aspect_ratio() {
        let rect = fit_rect(1_920, 1_080, 1_280, 1_024);
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0, 152, 1_280, 720)
        );
    }

    #[cfg(all(feature = "desktop-cpu", not(feature = "desktop-gpu")))]
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

    #[cfg(feature = "desktop-gpu")]
    #[test]
    fn bt709_limited_range_uses_video_range_offset() {
        let parameters = yuv_color_parameters(color::Space::BT709, color::Range::MPEG);
        assert_eq!(parameters[0], 1.1644);
        assert_eq!(parameters[12], -16.0 / 255.0);
        assert_eq!(parameters[13], -0.5);
    }

    #[cfg(feature = "desktop-gpu")]
    #[test]
    fn bt2020_uses_its_own_chroma_coefficients() {
        let full = yuv_color_parameters(color::Space::BT2020NCL, color::Range::JPEG);
        assert_eq!(full[6], 1.8814);
        assert_eq!(full[8], 1.4746);

        let limited = yuv_color_parameters(color::Space::BT2020NCL, color::Range::MPEG);
        assert_eq!(limited[6], 2.1418);
        assert_eq!(limited[8], 1.6787);
    }
}
