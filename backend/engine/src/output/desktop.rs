use std::{
    collections::VecDeque,
    ffi::c_void,
    mem::size_of,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use cosmic_text::Weight;
use ffmpeg_next::{Rational, Rescale, frame};
use sdl2::{
    Sdl, VideoSubsystem,
    audio::{AudioQueue, AudioSpecDesired},
    event::{Event, WindowEvent},
    keyboard::Keycode,
    mouse::MouseButton,
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::{BlendMode, Canvas, Texture},
    sys,
    video::{FullscreenType, Window},
};

use super::vtt;
use super::{FrameOutput, PlaybackStopped};
use crate::{
    analysis::audio_level::{AudioLevelCallback, AudioLevelMeter},
    audio_mixer::{AudioEffectChain, AudioEffectsControl},
    benchmark::{self, BenchHandle, Stage},
    compositor::{
        logo::{LogoOverlay, logo_position},
        text::{render_plain_text_bitmap, render_wrapped_text_bitmap},
    },
    utils::config::{OutputConfig, RgbaColor},
};

const AUDIO_CHANNELS: usize = 2;
const AUDIO_PREBUFFER_MS: u64 = 100;
const AUDIO_MAX_QUEUE_MS: u64 = 250;
const AUDIO_MAX_PENDING_MS: u64 = 500;
const VIDEO_PREBUFFER_FRAMES: usize = 3;
const VIDEO_DROP_THRESHOLD_FRAMES: i64 = 3;
const VIDEO_STARVATION_GRACE_FRAMES: i64 = 2;
const SCHEDULER_INTERVAL: Duration = Duration::from_millis(2);
const OUTPUT_CHANNEL_CAPACITY: usize = 8;
const DESKTOP_VOLUME_STEP: f64 = 0.05;
const DESKTOP_VOLUME_MIN: f64 = 0.0;
const DESKTOP_VOLUME_MAX: f64 = 1.5;
const VOLUME_OVERLAY_DURATION: Duration = Duration::from_millis(900);
const TITLEBAR_IDLE_TIMEOUT: Duration = Duration::from_secs(3);
const TITLEBAR_HEIGHT: u32 = 28;
const TITLEBAR_BACKGROUND_ALPHA: u8 = 180;
const TITLEBAR_CLOSE_MARGIN: u32 = 4;
const TITLEBAR_BUTTON_GAP: u32 = 4;
const TITLEBAR_TITLE: &str = "ffplayout";
const WINDOW_RESIZE_BORDER: i32 = 8;
const WINDOW_ASPECT_SETTLE: Duration = Duration::from_millis(120);
const SUBTITLE_FONT_SIZE: f32 = 24.0;
const SUBTITLE_FULLSCREEN_FONT_SIZE: f32 = 44.0;
const SUBTITLE_OUTLINE: u32 = 2;
const SUBTITLE_MARGIN_BOTTOM: u32 = 56;
const SUBTITLE_MAX_WIDTH_PERCENT: u32 = 92;
const DOUBLE_CLICK_MIN_INTERVAL: Duration = Duration::from_millis(80);
const DOUBLE_CLICK_MAX_INTERVAL: Duration = Duration::from_millis(450);
const DOUBLE_CLICK_MAX_DISTANCE: i32 = 6;

static HIT_TEST_STATE: OnceLock<Mutex<HitTestState>> = OnceLock::new();
static HIT_TEST_DOUBLE_CLICK: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default)]
struct HitTestState {
    last_at: Option<Instant>,
    last_x: i32,
    last_y: i32,
}

fn video_prebuffer_ready(queue_len: usize, video_finished: bool, force: bool) -> bool {
    force || video_finished || queue_len >= VIDEO_PREBUFFER_FRAMES
}

fn input_can_pause(video_queue_len: usize, video_finished: bool) -> bool {
    video_finished || video_queue_len >= VIDEO_PREBUFFER_FRAMES
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

enum DesktopMessage {
    ClipStarted,
    VideoWithLogo {
        frame: frame::Video,
        logo_opacity: f64,
    },
    Audio {
        samples: Vec<f32>,
        samples_per_channel: usize,
    },
    Subtitles(Vec<DesktopSubtitleCue>),
    VideoEnd(Option<i64>),
    VideoFinished,
    ClipFinished,
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
    sender: SyncSender<DesktopMessage>,
    discontinuity_sender: SyncSender<DesktopDiscontinuity>,
    audio_effects: Arc<Mutex<AudioEffectChain>>,
    audio_level_meter: AudioLevelMeter,
    current_logo_opacity: f64,
}

pub(crate) struct DesktopSdl {
    sdl: Sdl,
    video: VideoSubsystem,
}

struct DesktopRenderer {
    // Texture must be dropped before its parent canvas.
    texture: Texture,
    title_texture: Option<TitleTexture>,
    canvas: Canvas<Window>,
    audio: AudioQueue<f32>,
    audio_effects_control: AudioEffectsControl,
    event_pump: sdl2::EventPump,
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
    subtitle_texture: Option<TitleTexture>,
    logo_texture: Option<DesktopLogoTexture>,
    current_logo_opacity: f64,
    fullscreen: bool,
    aspect_width: u32,
    aspect_height: u32,
    last_window_size: (u32, u32),
    pending_aspect_resize: Option<(u32, u32, Instant)>,
    window_active: bool,
    mouse_over_window: bool,
    last_mouse_motion: Option<Instant>,
    volume_overlay_until: Option<Instant>,
    // SDL context must outlive all resources above.
    _sdl: Sdl,
}

struct TitleTexture {
    texture: Texture,
    width: u32,
    height: u32,
}

struct DesktopLogoTexture {
    texture: Texture,
    width: u32,
    height: u32,
    position: String,
    opacity: u8,
}

impl DesktopOutput {
    pub(super) fn open(cfg: &OutputConfig, desktop_sdl: DesktopSdl) -> Result<Self> {
        Ok(Self {
            renderer: DesktopRenderer::open(cfg, desktop_sdl)?,
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
        let (sender, receiver) = sync_channel(OUTPUT_CHANNEL_CAPACITY);
        let (discontinuity_sender, discontinuity_receiver) = sync_channel(1);
        let audio_effects = Arc::clone(&self.audio_effects);
        let audio_level_callback = self.audio_level_callback.clone();
        let audio_sample_rate = self.audio_sample_rate;
        let worker_benchmark = benchmark.clone();
        let worker = thread::Builder::new()
            .name("ffplayout-decode".to_string())
            .spawn(move || {
                benchmark::activate(worker_benchmark);
                let mut output = DesktopFrameSender {
                    sender,
                    discontinuity_sender,
                    audio_effects,
                    audio_level_meter: AudioLevelMeter::new(
                        audio_sample_rate,
                        audio_level_callback,
                    ),
                    current_logo_opacity: 0.0,
                };
                let _ = output.sender.send(DesktopMessage::ClipStarted);
                let result = operation(&mut output);
                let _ = output.sender.send(DesktopMessage::ClipFinished);
                result
            })
            .map_err(|error| anyhow!("failed to start decode worker: {error}"))?;

        benchmark::activate(benchmark.clone());
        let render_result = self.renderer.run_clip(receiver, discontinuity_receiver);
        if let Err(error) = render_result {
            if error.downcast_ref::<PlaybackStopped>().is_some() {
                benchmark::detach();
                thread::spawn(move || {
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
        benchmark::measure(Stage::DesktopOutput, || {
            self.sender.send(DesktopMessage::VideoWithLogo {
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

        benchmark::measure(Stage::DesktopOutput, || {
            self.sender
                .send(DesktopMessage::Audio {
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
        self.sender
            .send(DesktopMessage::VideoEnd(video_end_pts))
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
            .map(|cue| {
                let cue_start_ms = cue.start_ms.saturating_sub(source_start_ms);
                let cue_end_ms = cue.end_ms - source_start_ms;
                DesktopSubtitleCue {
                    start_ms: output_start_ms + cue_start_ms,
                    end_ms: output_start_ms + cue_end_ms,
                    text: cue.text,
                }
            })
            .collect();

        self.sender
            .send(DesktopMessage::Subtitles(subtitles))
            .map_err(|_| PlaybackStopped.into())
    }

    fn video_finished(&mut self) -> Result<()> {
        self.sender
            .send(DesktopMessage::VideoFinished)
            .map_err(|_| PlaybackStopped.into())
    }
}

impl DesktopRenderer {
    fn open(cfg: &OutputConfig, desktop_sdl: DesktopSdl) -> Result<Self> {
        let DesktopSdl { sdl, video } = desktop_sdl;
        let audio_subsystem = sdl.audio().map_err(anyhow::Error::msg)?;
        let mut window = video
            .window("ffplayout", cfg.width, cfg.height)
            .position_centered()
            .borderless()
            .resizable()
            .build()
            .map_err(|error| anyhow!("{error}"))?;
        if cfg.desktop_fullscreen {
            window
                .set_fullscreen(FullscreenType::Desktop)
                .map_err(|error| anyhow!("{error}"))?;
        }
        install_window_hit_test(&window);
        let mut canvas = window
            .into_canvas()
            .accelerated()
            .build()
            .map_err(|error| anyhow!("{error}"))?;
        canvas.set_blend_mode(BlendMode::Blend);
        let title_texture = create_title_texture(&canvas)?;
        let logo_texture = cfg
            .logo
            .as_ref()
            .map(|logo| create_desktop_logo_texture(&canvas, logo, cfg.width, cfg.height))
            .transpose()?;
        let texture = canvas
            .texture_creator()
            .create_texture_streaming(PixelFormatEnum::IYUV, cfg.width, cfg.height)
            .map_err(|error| anyhow!("{error}"))?;
        let desired = AudioSpecDesired {
            freq: Some(cfg.sample_rate as i32),
            channels: Some(2),
            samples: Some(1024),
        };
        let audio = audio_subsystem
            .open_queue::<f32, _>(None, &desired)
            .map_err(anyhow::Error::msg)?;
        if audio.spec().freq != cfg.sample_rate as i32
            || usize::from(audio.spec().channels) != AUDIO_CHANNELS
        {
            return Err(anyhow!(
                "SDL opened an incompatible audio format: {} Hz, {} channel(s)",
                audio.spec().freq,
                audio.spec().channels
            ));
        }
        let device_buffer_samples = u64::from(audio.spec().samples);
        let event_pump = sdl.event_pump().map_err(anyhow::Error::msg)?;

        Ok(Self {
            canvas,
            title_texture,
            audio,
            audio_effects_control: cfg.audio_effects.clone(),
            event_pump,
            video_queue: VecDeque::with_capacity(OUTPUT_CHANNEL_CAPACITY),
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
            subtitle_texture: None,
            logo_texture,
            current_logo_opacity: 0.0,
            fullscreen: cfg.desktop_fullscreen,
            aspect_width: cfg.width.max(1),
            aspect_height: cfg.height.max(1),
            last_window_size: (cfg.width, cfg.height),
            pending_aspect_resize: None,
            window_active: true,
            mouse_over_window: false,
            last_mouse_motion: None,
            volume_overlay_until: None,
            texture,
            _sdl: sdl,
        })
    }

    fn run_clip(
        &mut self,
        receiver: Receiver<DesktopMessage>,
        discontinuity_receiver: Receiver<DesktopDiscontinuity>,
    ) -> Result<()> {
        loop {
            self.handle_events()?;
            self.apply_pending_window_aspect_constraint()?;
            if let Ok(discontinuity) = discontinuity_receiver.try_recv() {
                self.apply_discontinuity(discontinuity);
                return Ok(());
            }
            self.flush_pending_audio()?;
            self.render_due_video()?;

            if self.pending_audio_samples >= self.max_pending_samples()
                && input_can_pause(self.video_queue.len(), self.video_finished)
            {
                thread::sleep(SCHEDULER_INTERVAL);
                continue;
            }

            let message = receiver.recv_timeout(SCHEDULER_INTERVAL);
            if let Ok(discontinuity) = discontinuity_receiver.try_recv() {
                self.apply_discontinuity(discontinuity);
                return Ok(());
            }

            match message {
                Ok(DesktopMessage::ClipStarted) => {
                    self.video_end_pts = None;
                    self.video_finished = false;
                    self.last_starvation_report = None;
                    self.subtitles.clear();
                    self.active_subtitle_text = None;
                    self.subtitle_texture = None;
                }
                Ok(DesktopMessage::VideoWithLogo {
                    frame,
                    logo_opacity,
                }) => {
                    self.current_logo_opacity = logo_opacity;
                    self.video_queue.push_back(frame);
                    self.start_audio_if_ready(false);
                }
                Ok(DesktopMessage::Audio {
                    samples,
                    samples_per_channel,
                }) => {
                    self.pending_audio.push_back((samples, samples_per_channel));
                    self.pending_audio_samples = self
                        .pending_audio_samples
                        .saturating_add(samples_per_channel as u64);
                    self.flush_pending_audio()?;
                }
                Ok(DesktopMessage::Subtitles(subtitles)) => {
                    self.subtitles = subtitles;
                    self.active_subtitle_text = None;
                    self.subtitle_texture = None;
                }
                Ok(DesktopMessage::VideoEnd(video_end_pts)) => {
                    self.video_end_pts = video_end_pts;
                }
                Ok(DesktopMessage::VideoFinished) => {
                    self.video_finished = true;
                    self.start_audio_if_ready(false);
                }
                Ok(DesktopMessage::ClipFinished) => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("decode worker disconnected"));
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
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
        self.subtitle_texture = None;
    }

    fn finish(mut self) -> Result<()> {
        self.flush_pending_audio()?;
        self.start_audio_if_ready(true);
        // Bound the drain phase: if the audio device stops consuming (e.g. it
        // was unplugged), `audio.size()` never reaches zero and this loop
        // would otherwise spin forever.
        let remaining_samples = self.pending_audio_samples + self.queued_audio_samples();
        let deadline = Instant::now()
            + Duration::from_secs_f64(remaining_samples as f64 / f64::from(self.sample_rate))
            + Duration::from_secs(2);
        while !self.pending_audio.is_empty() || self.audio.size() > 0 {
            if Instant::now() >= deadline {
                log::warn!("desktop audio did not drain in time; finishing playback anyway");
                break;
            }
            self.handle_events()?;
            self.flush_pending_audio()?;
            self.render_due_video()?;
            thread::sleep(SCHEDULER_INTERVAL);
        }
        thread::sleep(Duration::from_secs_f64(
            self.device_buffer_samples as f64 / f64::from(self.sample_rate),
        ));
        self.render_due_video_at(self.submitted_audio_samples)
    }

    fn flush_pending_audio(&mut self) -> Result<()> {
        while self.queued_audio_samples() < self.max_queue_samples() {
            let Some((samples, samples_per_channel)) = self.pending_audio.pop_front() else {
                break;
            };
            self.audio
                .queue_audio(&samples)
                .map_err(anyhow::Error::msg)?;
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

        let audio_pts = self.played_audio_samples();
        self.render_due_video_at(audio_pts)
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
            self.render_black_frame();
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

        benchmark::measure(Stage::DesktopOutput, || {
            self.texture
                .update_yuv(
                    None,
                    frame.data(0),
                    frame.stride(0),
                    frame.data(1),
                    frame.stride(1),
                    frame.data(2),
                    frame.stride(2),
                )
                .map_err(|error| anyhow!("{error}"))?;

            self.canvas.clear();
            self.canvas
                .copy(&self.texture, None, None)
                .map_err(anyhow::Error::msg)?;
            self.draw_logo_overlay()?;
            self.draw_subtitle_overlay(frame.pts().unwrap_or_default())?;
            self.draw_titlebar()?;
            self.draw_volume_overlay()?;
            self.canvas.present();
            Ok(())
        })
    }

    fn render_black_frame(&mut self) {
        self.canvas.set_draw_color(Color::RGB(0, 0, 0));
        self.canvas.clear();
        let _ = self.draw_logo_overlay();
        let _ = self.draw_subtitle_overlay(self.last_rendered_video_pts.unwrap_or_default());
        let _ = self.draw_titlebar();
        let _ = self.draw_volume_overlay();
        self.canvas.present();
    }

    fn queued_audio_samples(&self) -> u64 {
        u64::from(self.audio.size()) / (AUDIO_CHANNELS * size_of::<f32>()) as u64
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
        while let Some(event) = self.event_pump.poll_event() {
            match event {
                Event::Quit { .. } => return Err(PlaybackStopped.into()),
                Event::Window {
                    win_event: WindowEvent::FocusGained,
                    ..
                } => self.window_active = true,
                Event::Window {
                    win_event: WindowEvent::FocusLost,
                    ..
                } => self.window_active = false,
                Event::Window {
                    win_event: WindowEvent::Enter,
                    ..
                } => {
                    self.mouse_over_window = true;
                    self.last_mouse_motion = Some(Instant::now());
                }
                Event::Window {
                    win_event: WindowEvent::Leave,
                    ..
                } => {
                    self.mouse_over_window = false;
                    self.last_mouse_motion = None;
                }
                Event::Window {
                    win_event: WindowEvent::Resized(width, height),
                    ..
                }
                | Event::Window {
                    win_event: WindowEvent::SizeChanged(width, height),
                    ..
                } => {
                    self.queue_window_aspect_constraint(width, height);
                }
                Event::MouseMotion { .. } => {
                    self.last_mouse_motion = Some(Instant::now());
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    clicks,
                    x,
                    y,
                    ..
                } if self.titlebar_visible() => match titlebar_button_hit(
                    self.canvas.output_size().map_err(anyhow::Error::msg)?,
                    x,
                    y,
                ) {
                    Some(TitlebarButton::Close) => return Err(PlaybackStopped.into()),
                    Some(TitlebarButton::Fullscreen) => self.toggle_fullscreen()?,
                    Some(TitlebarButton::Minimize) => self.canvas.window_mut().minimize(),
                    None if clicks >= 2 || take_hit_test_double_click() => {
                        self.toggle_fullscreen()?;
                    }
                    None => {}
                },
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    clicks,
                    ..
                } if clicks >= 2 || take_hit_test_double_click() => self.toggle_fullscreen()?,
                Event::KeyDown {
                    keycode: Some(Keycode::F),
                    repeat: false,
                    ..
                } => self.toggle_fullscreen()?,
                Event::KeyDown {
                    keycode: Some(Keycode::S),
                    repeat: false,
                    ..
                } => self.toggle_subtitles(),
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    ..
                } => self.adjust_volume(-DESKTOP_VOLUME_STEP)?,
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                } => self.adjust_volume(DESKTOP_VOLUME_STEP)?,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return Err(PlaybackStopped.into()),
                _ => {}
            }
        }
        self.apply_pending_window_aspect_constraint()?;
        Ok(())
    }

    fn toggle_subtitles(&mut self) {
        self.subtitles_enabled = !self.subtitles_enabled;
        self.active_subtitle_text = None;
        self.subtitle_texture = None;
    }

    fn queue_window_aspect_constraint(&mut self, width: i32, height: i32) {
        if self.fullscreen || width <= 0 || height <= 0 {
            return;
        }

        self.pending_aspect_resize = Some((width as u32, height as u32, Instant::now()));
    }

    fn apply_pending_window_aspect_constraint(&mut self) -> Result<()> {
        let Some((width, height, at)) = self.pending_aspect_resize else {
            return Ok(());
        };
        if Instant::now().duration_since(at) < WINDOW_ASPECT_SETTLE {
            return Ok(());
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
            self.canvas
                .window_mut()
                .set_size(target.0, target.1)
                .map_err(|error| anyhow!("{error}"))?;
        }

        Ok(())
    }

    fn adjust_volume(&mut self, delta: f64) -> Result<()> {
        let volume = adjusted_volume(self.audio_effects_control.volume(), delta);
        self.audio_effects_control.set_volume(volume)?;
        self.volume_overlay_until = Some(Instant::now() + VOLUME_OVERLAY_DURATION);
        Ok(())
    }

    fn toggle_fullscreen(&mut self) -> Result<()> {
        self.set_fullscreen(!self.fullscreen)
    }

    fn set_fullscreen(&mut self, fullscreen: bool) -> Result<()> {
        let mode = if fullscreen {
            FullscreenType::Desktop
        } else {
            FullscreenType::Off
        };
        self.canvas
            .window_mut()
            .set_fullscreen(mode)
            .map_err(anyhow::Error::msg)?;
        self.fullscreen = fullscreen;
        self.active_subtitle_text = None;
        self.subtitle_texture = None;
        Ok(())
    }

    fn titlebar_visible(&self) -> bool {
        self.window_active
            && self.mouse_over_window
            && self
                .last_mouse_motion
                .is_some_and(|last| Instant::now().duration_since(last) < TITLEBAR_IDLE_TIMEOUT)
    }

    fn draw_subtitle_overlay(&mut self, video_pts: i64) -> Result<()> {
        self.update_subtitle_texture(video_pts)?;

        let Some(subtitle) = &self.subtitle_texture else {
            return Ok(());
        };

        let (width, height) = self.canvas.output_size().map_err(anyhow::Error::msg)?;
        let x = ((width.saturating_sub(subtitle.width)) / 2) as i32;
        let y = height.saturating_sub(subtitle.height + SUBTITLE_MARGIN_BOTTOM) as i32;

        self.canvas
            .copy(
                &subtitle.texture,
                None,
                Rect::new(x, y, subtitle.width, subtitle.height),
            )
            .map_err(anyhow::Error::msg)
    }

    fn update_subtitle_texture(&mut self, video_pts: i64) -> Result<()> {
        let text = self.active_subtitle_for_pts(video_pts);

        if self.active_subtitle_text.as_deref() == text.as_deref() {
            return Ok(());
        }

        self.active_subtitle_text = text.clone();
        self.subtitle_texture = match text {
            Some(text) => Some(create_subtitle_texture(
                &self.canvas,
                &text,
                self.fullscreen,
            )?),
            None => None,
        };

        Ok(())
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

    fn draw_logo_overlay(&mut self) -> Result<()> {
        let (canvas_width, canvas_height) =
            self.canvas.output_size().map_err(anyhow::Error::msg)?;
        let (window_width, window_height) = self.canvas.window().size();
        let Some(logo) = &mut self.logo_texture else {
            return Ok(());
        };

        let alpha = (f64::from(logo.opacity) * self.current_logo_opacity)
            .round()
            .clamp(0.0, 255.0) as u8;
        if alpha == 0 {
            return Ok(());
        }

        let (x, y) = logo_position(
            &logo.position,
            window_width,
            window_height,
            logo.width,
            logo.height,
        )?;
        let scale_x = f64::from(canvas_width) / f64::from(window_width.max(1));
        let scale_y = f64::from(canvas_height) / f64::from(window_height.max(1));
        let dst = Rect::new(
            (f64::from(x) * scale_x).round() as i32,
            (f64::from(y) * scale_y).round() as i32,
            (f64::from(logo.width) * scale_x).round().max(1.0) as u32,
            (f64::from(logo.height) * scale_y).round().max(1.0) as u32,
        );
        logo.texture.set_alpha_mod(alpha);
        self.canvas
            .copy(&logo.texture, None, dst)
            .map_err(anyhow::Error::msg)
    }

    fn draw_titlebar(&mut self) -> Result<()> {
        if !self.titlebar_visible() {
            return Ok(());
        }

        let (width, _) = self.canvas.output_size().map_err(anyhow::Error::msg)?;
        let titlebar = Rect::new(0, 0, width, TITLEBAR_HEIGHT);

        self.canvas
            .set_draw_color(Color::RGBA(14, 16, 18, TITLEBAR_BACKGROUND_ALPHA));
        self.canvas
            .fill_rect(titlebar)
            .map_err(anyhow::Error::msg)?;
        self.draw_titlebar_text(width)?;
        self.canvas.set_draw_color(Color::RGB(218, 218, 218));
        self.draw_minimize_icon(titlebar_button_rect(width, TitlebarButton::Minimize))?;
        self.draw_fullscreen_icon(titlebar_button_rect(width, TitlebarButton::Fullscreen))?;
        self.draw_close_icon(titlebar_button_rect(width, TitlebarButton::Close))?;
        Ok(())
    }

    fn draw_titlebar_text(&mut self, width: u32) -> Result<()> {
        let Some(title) = &self.title_texture else {
            return Ok(());
        };

        let x = ((width.saturating_sub(title.width)) / 2) as i32;
        let y = ((TITLEBAR_HEIGHT.saturating_sub(title.height)) / 2) as i32 + 2;
        let first_button = titlebar_button_rect(width, TitlebarButton::Minimize);

        if x < 10 || x + title.width as i32 >= first_button.x - 10 {
            return Ok(());
        }

        self.canvas
            .copy(
                &title.texture,
                None,
                Rect::new(x, y, title.width, title.height),
            )
            .map_err(anyhow::Error::msg)
    }

    fn draw_minimize_icon(&mut self, rect: Rect) -> Result<()> {
        let y = rect.y + rect.height() as i32 - 7;
        self.canvas
            .fill_rect(Rect::new(rect.x + 5, y, rect.width() - 10, 2))
            .map_err(anyhow::Error::msg)
    }

    fn draw_fullscreen_icon(&mut self, rect: Rect) -> Result<()> {
        let x = rect.x + 5;
        let y = rect.y + 5;
        let size = rect.width() - 10;

        self.canvas
            .draw_rect(Rect::new(x, y, size, size))
            .map_err(anyhow::Error::msg)?;
        self.canvas
            .draw_rect(Rect::new(
                x + 1,
                y + 1,
                size.saturating_sub(2),
                size.saturating_sub(2),
            ))
            .map_err(anyhow::Error::msg)
    }

    fn draw_close_icon(&mut self, rect: Rect) -> Result<()> {
        let left = rect.x + 6;
        let right = rect.x + rect.width() as i32 - 6;
        let top = rect.y + 6;
        let bottom = rect.y + rect.height() as i32 - 6;

        self.canvas
            .draw_line((left, top), (right, bottom))
            .map_err(anyhow::Error::msg)?;
        self.canvas
            .draw_line((left + 1, top), (right + 1, bottom))
            .map_err(anyhow::Error::msg)?;
        self.canvas
            .draw_line((right, top), (left, bottom))
            .map_err(anyhow::Error::msg)?;
        self.canvas
            .draw_line((right + 1, top), (left + 1, bottom))
            .map_err(anyhow::Error::msg)
    }

    fn draw_volume_overlay(&mut self) -> Result<()> {
        if self
            .volume_overlay_until
            .is_none_or(|until| Instant::now() >= until)
        {
            self.volume_overlay_until = None;
            return Ok(());
        }

        let (width, height) = self.canvas.output_size().map_err(anyhow::Error::msg)?;
        let panel_width = (width / 3).clamp(180, 360);
        let panel_height = 28;
        let margin = 24;
        let x = ((width - panel_width) / 2) as i32;
        let y = height.saturating_sub(panel_height + margin) as i32;
        let panel = Rect::new(x, y, panel_width, panel_height);
        let track_margin = 8;
        let track_height = 6;
        let track_width = panel_width - track_margin * 2;
        let track_x = x + track_margin as i32;
        let track_y = y + ((panel_height - track_height) / 2) as i32;
        let fill_width = ((self.audio_effects_control.volume() / DESKTOP_VOLUME_MAX)
            .clamp(0.0, 1.0)
            * f64::from(track_width))
        .round() as u32;

        self.canvas.set_draw_color(Color::RGB(16, 18, 20));
        self.canvas.fill_rect(panel).map_err(anyhow::Error::msg)?;
        self.canvas.set_draw_color(Color::RGB(78, 86, 96));
        self.canvas
            .fill_rect(Rect::new(track_x, track_y, track_width, track_height))
            .map_err(anyhow::Error::msg)?;
        self.canvas.set_draw_color(Color::RGB(238, 238, 238));
        self.canvas
            .fill_rect(Rect::new(track_x, track_y, fill_width, track_height))
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }
}

fn install_window_hit_test(window: &Window) {
    // Best effort: unsupported platforms simply keep the borderless window
    // without compositor-assisted dragging.
    unsafe {
        sys::SDL_SetWindowHitTest(window.raw(), Some(desktop_hit_test), std::ptr::null_mut());
    }
}

extern "C" fn desktop_hit_test(
    window: *mut sys::SDL_Window,
    area: *const sys::SDL_Point,
    _data: *mut c_void,
) -> sys::SDL_HitTestResult {
    if window.is_null() || area.is_null() {
        return sys::SDL_HitTestResult::SDL_HITTEST_NORMAL;
    }

    let point = unsafe { *area };
    let mut width = 0;
    let mut height = 0;
    unsafe {
        sys::SDL_GetWindowSize(window, &mut width, &mut height);
    }

    if let Some(resize) = resize_hit_test(window, width, height, point.x, point.y) {
        return resize;
    }

    if titlebar_button_hit(
        (width.max(0) as u32, height.max(0) as u32),
        point.x,
        point.y,
    )
    .is_some()
    {
        return sys::SDL_HitTestResult::SDL_HITTEST_NORMAL;
    }

    if detect_hit_test_double_click(point.x, point.y) {
        HIT_TEST_DOUBLE_CLICK.store(true, Ordering::Relaxed);
        return sys::SDL_HitTestResult::SDL_HITTEST_NORMAL;
    }

    sys::SDL_HitTestResult::SDL_HITTEST_DRAGGABLE
}

fn resize_hit_test(
    window: *mut sys::SDL_Window,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
) -> Option<sys::SDL_HitTestResult> {
    if width <= 0 || height <= 0 {
        return None;
    }

    let flags = unsafe { sys::SDL_GetWindowFlags(window) };
    if flags & sys::SDL_WindowFlags::SDL_WINDOW_FULLSCREEN_DESKTOP as u32 != 0 {
        return None;
    }

    let left = x < WINDOW_RESIZE_BORDER;
    let right = x >= width.saturating_sub(WINDOW_RESIZE_BORDER);
    let top = y < WINDOW_RESIZE_BORDER;
    let bottom = y >= height.saturating_sub(WINDOW_RESIZE_BORDER);

    match (left, right, top, bottom) {
        (true, _, true, _) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_TOPLEFT),
        (_, true, true, _) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_TOPRIGHT),
        (true, _, _, true) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_BOTTOMLEFT),
        (_, true, _, true) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_BOTTOMRIGHT),
        (true, _, _, _) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_LEFT),
        (_, true, _, _) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_RIGHT),
        (_, _, true, _) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_TOP),
        (_, _, _, true) => Some(sys::SDL_HitTestResult::SDL_HITTEST_RESIZE_BOTTOM),
        _ => None,
    }
}

fn detect_hit_test_double_click(x: i32, y: i32) -> bool {
    let now = Instant::now();
    let state = HIT_TEST_STATE.get_or_init(|| Mutex::new(HitTestState::default()));
    let Ok(mut state) = state.lock() else {
        return false;
    };

    let is_double_click = state.last_at.is_some_and(|last| {
        let elapsed = now.duration_since(last);
        elapsed >= DOUBLE_CLICK_MIN_INTERVAL
            && elapsed <= DOUBLE_CLICK_MAX_INTERVAL
            && (state.last_x - x).abs() <= DOUBLE_CLICK_MAX_DISTANCE
            && (state.last_y - y).abs() <= DOUBLE_CLICK_MAX_DISTANCE
    });

    state.last_at = Some(now);
    state.last_x = x;
    state.last_y = y;
    is_double_click
}

fn take_hit_test_double_click() -> bool {
    HIT_TEST_DOUBLE_CLICK.swap(false, Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy)]
enum TitlebarButton {
    Minimize,
    Fullscreen,
    Close,
}

fn titlebar_button_rect(window_width: u32, button: TitlebarButton) -> Rect {
    let size = TITLEBAR_HEIGHT.saturating_sub(TITLEBAR_CLOSE_MARGIN * 2);
    let index_from_right = match button {
        TitlebarButton::Close => 0,
        TitlebarButton::Fullscreen => 1,
        TitlebarButton::Minimize => 2,
    };
    let offset = TITLEBAR_CLOSE_MARGIN + index_from_right * (size + TITLEBAR_BUTTON_GAP);
    let x = window_width.saturating_sub(size + offset) as i32;

    Rect::new(x, TITLEBAR_CLOSE_MARGIN as i32, size, size)
}

fn titlebar_button_hit((window_width, _): (u32, u32), x: i32, y: i32) -> Option<TitlebarButton> {
    [
        TitlebarButton::Minimize,
        TitlebarButton::Fullscreen,
        TitlebarButton::Close,
    ]
    .into_iter()
    .find(|button| titlebar_button_rect(window_width, *button).contains_point((x, y)))
}

fn create_title_texture(canvas: &Canvas<Window>) -> Result<Option<TitleTexture>> {
    Ok(Some(create_text_texture(
        canvas,
        TITLEBAR_TITLE,
        14.0,
        Weight::SEMIBOLD,
        RgbaColor {
            r: 218,
            g: 218,
            b: 218,
            a: 255,
        },
    )?))
}

fn create_desktop_logo_texture(
    canvas: &Canvas<Window>,
    config: &crate::utils::config::LogoConfig,
    output_width: u32,
    output_height: u32,
) -> Result<DesktopLogoTexture> {
    let logo = LogoOverlay::load(config, output_width, output_height)?;
    let pixels = yuva420p_to_rgba(&logo);
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_static(PixelFormatEnum::RGBA32, logo.width, logo.height)
        .map_err(|error| anyhow!("{error}"))?;

    texture.set_blend_mode(BlendMode::Blend);
    texture
        .update(None, &pixels, logo.width as usize * 4)
        .map_err(|error| anyhow!("{error}"))?;

    Ok(DesktopLogoTexture {
        texture,
        width: logo.width,
        height: logo.height,
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
            let yy = i32::from(y_plane[y * y_stride + x]);
            let uu = i32::from(u_plane[(y / 2) * u_stride + (x / 2)]);
            let vv = i32::from(v_plane[(y / 2) * v_stride + (x / 2)]);
            let aa = a_plane[y * a_stride + x];
            let (r, g, b) = yuv_to_rgb(yy, uu, vv);
            let dst = (y * width + x) * 4;
            pixels[dst] = r;
            pixels[dst + 1] = g;
            pixels[dst + 2] = b;
            pixels[dst + 3] = aa;
        }
    }

    pixels
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

fn create_subtitle_texture(
    canvas: &Canvas<Window>,
    text: &str,
    fullscreen: bool,
) -> Result<TitleTexture> {
    let (window_width, _) = canvas.output_size().map_err(anyhow::Error::msg)?;
    let max_width = (window_width * SUBTITLE_MAX_WIDTH_PERCENT / 100).max(1);
    let font_size = if fullscreen {
        SUBTITLE_FULLSCREEN_FONT_SIZE
    } else {
        SUBTITLE_FONT_SIZE
    };
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
            if distance > outline as f32 + 0.25 {
                continue;
            }
            let alpha = (1.0 - (distance / (outline as f32 + 0.75))).clamp(0.25, 1.0);
            composite_bitmap(&mut pixels, width, &black, outline + x, outline + y, alpha);
        }
    }
    composite_bitmap(
        &mut pixels,
        width,
        &white,
        SUBTITLE_OUTLINE as i32,
        SUBTITLE_OUTLINE as i32,
        1.0,
    );

    create_texture_from_rgba(canvas, pixels, width, height)
}

fn create_text_texture(
    canvas: &Canvas<Window>,
    text: &str,
    font_size: f32,
    font_weight: Weight,
    color: RgbaColor,
) -> Result<TitleTexture> {
    let bitmap = render_plain_text_bitmap(text, font_size, font_weight, color)?;
    create_texture_from_rgba(canvas, bitmap.pixels, bitmap.width, bitmap.height)
}

fn create_texture_from_rgba(
    canvas: &Canvas<Window>,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<TitleTexture> {
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_static(PixelFormatEnum::RGBA32, width, height)
        .map_err(|error| anyhow!("{error}"))?;

    texture.set_blend_mode(BlendMode::Blend);
    texture
        .update(None, &pixels, width as usize * 4)
        .map_err(|error| anyhow!("{error}"))?;

    Ok(TitleTexture {
        texture,
        width,
        height,
    })
}

fn composite_bitmap(
    dst: &mut [u8],
    dst_width: u32,
    src: &crate::compositor::text::TextBitmap,
    x: i32,
    y: i32,
    alpha_scale: f32,
) {
    let dst_width = dst_width as usize;
    for sy in 0..src.height as usize {
        let dy = y + sy as i32;
        if dy < 0 {
            continue;
        }

        for sx in 0..src.width as usize {
            let dx = x + sx as i32;
            if dx < 0 {
                continue;
            }

            let dst_idx = (dy as usize * dst_width + dx as usize) * 4;
            let src_idx = (sy * src.width as usize + sx) * 4;
            if dst_idx + 4 <= dst.len() {
                let src_pixel = [
                    src.pixels[src_idx],
                    src.pixels[src_idx + 1],
                    src.pixels[src_idx + 2],
                    ((src.pixels[src_idx + 3] as f32) * alpha_scale).round() as u8,
                ];
                alpha_composite_rgba(&mut dst[dst_idx..dst_idx + 4], &src_pixel);
            }
        }
    }
}

fn alpha_composite_rgba(dst: &mut [u8], src: &[u8]) {
    let alpha = u16::from(src[3]);
    let inv_alpha = 255 - alpha;
    dst[0] = (((u16::from(dst[0]) * inv_alpha) + (u16::from(src[0]) * alpha) + 127) / 255) as u8;
    dst[1] = (((u16::from(dst[1]) * inv_alpha) + (u16::from(src[1]) * alpha) + 127) / 255) as u8;
    dst[2] = (((u16::from(dst[2]) * inv_alpha) + (u16::from(src[2]) * alpha) + 127) / 255) as u8;
    dst[3] = (u16::from(dst[3]) + alpha - ((u16::from(dst[3]) * alpha + 127) / 255)) as u8;
}

pub(crate) fn init_sdl() -> Result<DesktopSdl> {
    // All desktop-output sessions (across restarts and channels) are routed
    // through the single persistent thread in `sdl_thread`, so SDL resources
    // always live on one OS thread. Wayland windows are created borderless and
    // get a small in-canvas titlebar instead of libdecor-provided decorations,
    // which avoids libdecor/GTK initialization failures on non-main threads.
    //
    // By default SDL installs its own SIGINT/SIGTERM handlers and turns them
    // into an `Event::Quit`, which `handle_events` treats as "stop the current
    // clip" (the same as closing the window or pressing Escape) rather than as
    // a request to terminate the whole process. Since the channel supervisor
    // automatically restarts a stopped channel, Ctrl-C would just restart
    // playback instead of exiting the process. Disabling SDL's signal handlers
    // restores the normal OS default (Ctrl-C terminates the process), matching
    // the behavior of the other output modes, which don't use SDL at all.
    sdl2::hint::set("SDL_NO_SIGNAL_HANDLERS", "1");
    sdl2::hint::set("SDL_VIDEO_WAYLAND_ALLOW_LIBDECOR", "0");
    sdl2::hint::set("SDL_VIDEO_WAYLAND_PREFER_LIBDECOR", "0");
    let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
    let video = sdl.video().map_err(anyhow::Error::msg)?;
    Ok(DesktopSdl { sdl, video })
}

pub(crate) fn config_for_primary_display(
    config: OutputConfig,
    _desktop_sdl: &DesktopSdl,
) -> OutputConfig {
    config
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
    fn audio_clock_reanchors_when_sdl_requests_another_buffer() {
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
    fn waits_for_video_prebuffer_before_starting_audio() {
        assert!(!video_prebuffer_ready(2, false, false));
        assert!(video_prebuffer_ready(3, false, false));
        assert!(video_prebuffer_ready(0, true, false));
    }

    #[test]
    fn backpressure_never_starves_an_active_video_stream() {
        assert!(!input_can_pause(0, false));
        assert!(!input_can_pause(2, false));
        assert!(input_can_pause(3, false));
        assert!(input_can_pause(0, true));
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
}
