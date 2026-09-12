use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    },
    thread as std_thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{Rational, Rescale, frame};
#[cfg(target_os = "linux")]
use winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    platform::pump_events::EventLoopExtPumpEvents,
    window::{Fullscreen, Icon, Window, WindowId},
};

use super::{EncodedOutput, FrameOutput, PlaybackStopped, vtt};
use crate::{
    analysis::{
        audio_level::{AudioLevelCallback, AudioLevelMeter},
        loudness::LoudnessMeter,
    },
    audio_mixer::{AudioEffectChain, AudioEffectsControl},
    benchmark::{self, BenchHandle, Stage},
    compositor::logo::{LogoOverlay, blend_logo},
    utils::{
        config::{DesktopControlCallback, DesktopControlCommand, OutputConfig},
        ffmpeg::reference_video_frame,
    },
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
const AUDIO_BUFFER_POOL_CAPACITY: usize = AUDIO_CHANNEL_CAPACITY;
const AUDIO_BUFFER_MAX_RETAINED_CAPACITY: usize =
    AUDIO_DEVICE_BUFFER_SAMPLES as usize * AUDIO_CHANNELS * 4;
const RECORDING_CHANNEL_CAPACITY: usize = 64;
const RECORDING_OVERLOAD_LOG_INTERVAL: Duration = Duration::from_secs(30);
const DESKTOP_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

const DESKTOP_WINDOW_TITLE: &str = "ffplayout";

#[cfg(target_os = "linux")]
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
    loudness_meter_control: crate::LoudnessMeterControl,
    audio_sample_rate: u32,
    recording: Option<DesktopRecording>,
    channel_id: Option<i32>,
}

enum DesktopControlMessage {
    ClipStarted,
    Subtitles(Vec<DesktopSubtitleCue>),
    VideoEnd(Option<i64>),
    VideoDecoded,
    VideoFinished,
    ClipFinished,
}

struct DesktopVideoMessage {
    frame: frame::Video,
    logo_opacity: f64,
}

enum DesktopAudioMessage {
    Samples {
        samples: Vec<f32>,
        samples_per_channel: usize,
    },
    Padding(u64),
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
    next_audio_pts: i64,
    video_sender: SyncSender<DesktopVideoMessage>,
    audio_sender: SyncSender<DesktopAudioMessage>,
    control_sender: SyncSender<DesktopControlMessage>,
    discontinuity_sender: SyncSender<DesktopDiscontinuity>,
    audio_effects: Arc<Mutex<AudioEffectChain>>,
    audio_buffer_pool: Arc<Mutex<Vec<Vec<f32>>>>,
    audio_level_meter: AudioLevelMeter,
    loudness_meter: LoudnessMeter,
    current_logo_opacity: f64,
    recording_sender: Option<SyncSender<DesktopRecordingMessage>>,
    recording_active: Option<Arc<AtomicBool>>,
    recording_queue_depth: Option<Arc<AtomicUsize>>,
    recording_logo: Option<Arc<LogoOverlay>>,
    recording_dropped_messages: u64,
    recording_last_overload_log: Option<Instant>,
    channel_id: i32,
}

struct DesktopRecording {
    sender: SyncSender<DesktopRecordingMessage>,
    worker: std_thread::JoinHandle<()>,
    active: Arc<AtomicBool>,
    queue_depth: Arc<AtomicUsize>,
}

enum DesktopRecordingMessage {
    Video {
        frame: frame::Video,
        logo: Option<Arc<LogoOverlay>>,
        logo_opacity: f64,
    },
    Audio(frame::Audio),
    AudioPadding {
        pts: i64,
        samples: u64,
    },
    Finish,
}

fn recording_silence(pts: i64, samples: u64, rate: u32) -> impl Iterator<Item = frame::Audio> {
    (0..samples).step_by(1_024).map(move |offset| {
        let count = (samples - offset).min(1_024) as usize;
        let mut frame = frame::Audio::new(
            ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
            count,
            ffmpeg_next::ChannelLayout::STEREO,
        );
        frame.set_rate(rate);
        frame.set_pts(Some(pts + offset as i64));
        for channel in 0..2 {
            frame.plane_mut::<f32>(channel).fill(0.0);
        }
        frame
    })
}

impl DesktopRecording {
    fn open(cfg: &OutputConfig, recording_config: &crate::RecordingConfig) -> Result<Self> {
        let (sender, receiver) = sync_channel(RECORDING_CHANNEL_CAPACITY);
        let (ready_sender, ready_receiver) = sync_channel(1);
        let cfg = cfg.clone();
        let recording_config = recording_config.clone();
        let channel_id = cfg.channel_id.unwrap_or_default();
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = Arc::clone(&active);
        let queue_depth = Arc::new(AtomicUsize::new(0));
        let worker_queue_depth = Arc::clone(&queue_depth);
        let worker = std_thread::Builder::new()
            .name("ffplayout-desktop-recording".to_string())
            .spawn(move || {
                let mut output = match EncodedOutput::open_recording(&cfg, &recording_config) {
                    Ok(output) => {
                        let _ = ready_sender.send(Ok(()));
                        output
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(error.to_string()));
                        return;
                    }
                };
                while let Ok(message) = receiver.recv() {
                    let result = match message {
                        DesktopRecordingMessage::Video {
                            mut frame,
                            logo,
                            logo_opacity,
                        } => {
                            worker_queue_depth.fetch_sub(1, Ordering::AcqRel);
                            if let Some(logo) = logo {
                                // A referenced frame is read-only across threads. Make the
                                // full copy in this worker only when compositing is required.
                                let mut composited = frame.clone();
                                blend_logo(&mut composited, &logo, logo_opacity);
                                frame = composited;
                            }
                            output.encode_video(&frame)
                        }
                        DesktopRecordingMessage::Audio(frame) => {
                            worker_queue_depth.fetch_sub(1, Ordering::AcqRel);
                            output.encode_audio(&frame)
                        }
                        DesktopRecordingMessage::AudioPadding { pts, samples } => {
                            worker_queue_depth.fetch_sub(1, Ordering::AcqRel);
                            recording_silence(pts, samples, cfg.sample_rate)
                                .try_for_each(|frame| output.encode_audio(&frame))
                        }
                        DesktopRecordingMessage::Finish => break,
                    };
                    if let Err(error) = result {
                        worker_active.store(false, Ordering::Release);
                        log::error!(channel = channel_id; "Recording disabled: {error}");
                        return;
                    }
                    if !worker_active.load(Ordering::Acquire) {
                        break;
                    }
                }
                if let Err(error) = output.finish() {
                    log::error!(channel = channel_id; "Recording disabled: {error}");
                }
            })
            .map_err(|error| anyhow!("failed to start desktop recording worker: {error}"))?;

        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                sender,
                worker,
                active,
                queue_depth,
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(anyhow!(error))
            }
            Err(_) => {
                let _ = worker.join();
                Err(anyhow!(
                    "desktop recording worker stopped during initialization"
                ))
            }
        }
    }

    fn finish(self) {
        let Self {
            sender,
            worker,
            active,
            queue_depth: _,
        } = self;
        // Do not make an output switch wait for decoded frames which have not
        // reached the recording encoder yet. The worker finishes its current
        // message and then writes the trailer.
        active.store(false, Ordering::Release);
        match sender.try_send(DesktopRecordingMessage::Finish) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                // Dropping the final sender lets the worker drain queued frames
                // before it closes the muxer and writes the trailer.
            }
        }
        drop(sender);
        if worker.join().is_err() {
            log::warn!("desktop recording worker panicked");
        }
    }
}

struct DesktopRenderer {
    window: Option<DesktopWindowHandle>,
    audio: DesktopAudio,
    audio_effects_control: AudioEffectsControl,
    video_queue: VecDeque<frame::Video>,
    pending_audio: VecDeque<DesktopAudioMessage>,
    audio_buffer_pool: Arc<Mutex<Vec<Vec<f32>>>>,
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
        let renderer = DesktopRenderer::open(cfg)?;
        let recording = cfg.recording.as_ref().and_then(|recording_config| {
            if recording_config.encode.is_none() {
                log::error!(channel = cfg.channel_id.unwrap_or_default(); "Recording disabled: desktop output requires dedicated recording encode settings");
                return None;
            }
            match DesktopRecording::open(cfg, recording_config) {
                Ok(recording) => Some(recording),
                Err(error) => {
                    log::error!(channel = cfg.channel_id.unwrap_or_default(); "Recording disabled: {error}");
                    None
                }
            }
        });
        Ok(Self {
            renderer,
            audio_effects: Arc::new(Mutex::new(AudioEffectChain::new(
                cfg.audio_effects.clone(),
                cfg.sample_rate,
            ))),
            audio_level_callback: cfg.audio_level_callback.clone(),
            loudness_meter_control: cfg.loudness_meter_control.clone(),
            audio_sample_rate: cfg.sample_rate,
            recording,
            channel_id: cfg.channel_id,
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
        let audio_buffer_pool = Arc::clone(&self.renderer.audio_buffer_pool);
        let audio_level_callback = self.audio_level_callback.clone();
        let loudness_meter_control = self.loudness_meter_control.clone();
        let audio_sample_rate = self.audio_sample_rate;
        let next_audio_pts =
            self.renderer
                .submitted_audio_samples
                .saturating_add(self.renderer.pending_audio_samples)
                .saturating_add(self.renderer.pending_silence_samples) as i64;
        let worker_benchmark = benchmark.clone();
        let recording_sender = self
            .recording
            .as_ref()
            .map(|recording| recording.sender.clone());
        let recording_active = self
            .recording
            .as_ref()
            .map(|recording| Arc::clone(&recording.active));
        let recording_queue_depth = self
            .recording
            .as_ref()
            .map(|recording| Arc::clone(&recording.queue_depth));
        let channel_id = self.channel_id.unwrap_or_default();
        let worker = std_thread::Builder::new()
            .name("ffplayout-decode".to_string())
            .spawn(move || {
                benchmark::activate(worker_benchmark);
                let mut output = DesktopFrameSender {
                    next_audio_pts,
                    video_sender,
                    audio_sender,
                    control_sender,
                    discontinuity_sender,
                    audio_effects,
                    audio_buffer_pool,
                    audio_level_meter: AudioLevelMeter::new(
                        audio_sample_rate,
                        audio_level_callback,
                    ),
                    loudness_meter: LoudnessMeter::new(audio_sample_rate, loudness_meter_control),
                    current_logo_opacity: 0.0,
                    recording_sender,
                    recording_active,
                    recording_queue_depth,
                    recording_logo: None,
                    recording_dropped_messages: 0,
                    recording_last_overload_log: None,
                    channel_id,
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

    pub(super) fn finish(mut self) -> Result<()> {
        let recording = self.recording.take();
        // Finish and drop the renderer first. Its Drop implementation destroys
        // the native window before a recording worker is joined.
        let renderer_result = self.renderer.finish();
        if let Some(recording) = recording {
            recording.finish();
        }
        renderer_result
    }
}

impl FrameOutput for DesktopFrameSender {
    fn audio_frame_size(&self) -> usize {
        1024
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        let desktop_frame = reference_video_frame(frame)?;
        let recording_slot = self.reserve_recording_slot();
        let desktop_send = benchmark::measure(Stage::DesktopSend, || {
            self.video_sender.send(DesktopVideoMessage {
                frame: desktop_frame,
                logo_opacity: self.current_logo_opacity,
            })
        });
        if desktop_send.is_err() {
            if recording_slot {
                self.release_recording_slot();
            }
            return Err(PlaybackStopped.into());
        }
        if recording_slot {
            match reference_video_frame(frame) {
                Ok(frame) => self.send_reserved_recording(DesktopRecordingMessage::Video {
                    frame,
                    logo: self.recording_logo.clone(),
                    logo_opacity: self.current_logo_opacity,
                }),
                Err(error) => {
                    self.release_recording_slot();
                    log::warn!(channel = self.channel_id; "Skipping desktop recording frame: {error}");
                }
            }
        }
        Ok(())
    }

    fn try_encode_video(&mut self, frame: &frame::Video) -> Result<bool> {
        match self.video_sender.try_send(DesktopVideoMessage {
            frame: reference_video_frame(frame)?,
            logo_opacity: self.current_logo_opacity,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Ok(false),
            Err(TrySendError::Disconnected(_)) => return Err(PlaybackStopped.into()),
        }
        if self.reserve_recording_slot() {
            match reference_video_frame(frame) {
                Ok(frame) => self.send_reserved_recording(DesktopRecordingMessage::Video {
                    frame,
                    logo: self.recording_logo.clone(),
                    logo_opacity: self.current_logo_opacity,
                }),
                Err(error) => {
                    self.release_recording_slot();
                    log::warn!(channel = self.channel_id; "Skipping desktop recording frame: {error}");
                }
            }
        }
        Ok(true)
    }

    fn apply_logo_overlay(
        &mut self,
        _frame: &mut frame::Video,
        logo: &LogoOverlay,
        opacity_factor: f64,
    ) {
        self.current_logo_opacity = opacity_factor;
        if self.recording_sender.is_some() && self.recording_logo.is_none() {
            self.recording_logo = Some(Arc::new(logo.clone()));
        }
    }

    fn benchmarks_logo_overlay(&self) -> bool {
        false
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        if frame.samples() == 0 {
            return Ok(());
        }

        let (samples, samples_per_channel, recording_frame) =
            benchmark::measure(Stage::AudioProcess, || {
                let mut frame = frame.clone();
                self.audio_effects
                    .lock()
                    .map_err(|_| anyhow!("audio effect chain lock poisoned"))?
                    .process(&mut frame);
                self.audio_level_meter.process_frame(&frame);
                self.loudness_meter.process_frame(&frame);
                let left = frame.plane::<f32>(0);
                let right = frame.plane::<f32>(1);
                let mut interleaved =
                    take_audio_buffer(&self.audio_buffer_pool, frame.samples() * AUDIO_CHANNELS);
                for (left, right) in left.iter().zip(right) {
                    interleaved.push(if left.is_finite() { *left } else { 0.0 });
                    interleaved.push(if right.is_finite() { *right } else { 0.0 });
                }
                let samples = frame.samples();
                Ok::<_, anyhow::Error>((interleaved, samples, frame))
            })?;

        benchmark::measure(Stage::DesktopSend, || {
            self.audio_sender
                .send(DesktopAudioMessage::Samples {
                    samples,
                    samples_per_channel,
                })
                .map_err(|_| anyhow::Error::new(PlaybackStopped))
        })?;
        self.next_audio_pts = frame.pts().unwrap_or(self.next_audio_pts) + frame.samples() as i64;
        if self.reserve_recording_slot() {
            self.send_reserved_recording(DesktopRecordingMessage::Audio(recording_frame));
        }
        Ok(())
    }

    fn reset_after_skip(&mut self, video_pts: i64, audio_pts: i64) -> Result<bool> {
        self.discontinuity_sender
            .send(DesktopDiscontinuity {
                video_pts,
                audio_pts,
            })
            .map_err(|_| PlaybackStopped)?;
        self.next_audio_pts = audio_pts;
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

    fn clear_vtt_subtitles(&mut self) -> Result<()> {
        self.control_sender
            .send(DesktopControlMessage::Subtitles(Vec::new()))
            .map_err(|_| PlaybackStopped.into())
    }

    fn video_finished(&mut self) -> Result<()> {
        self.control_sender
            .send(DesktopControlMessage::VideoFinished)
            .map_err(|_| PlaybackStopped.into())
    }

    fn pad_audio(&mut self, samples: i64) -> Result<bool> {
        let samples = u64::try_from(samples).map_err(|_| anyhow!("negative audio padding"))?;
        self.audio_sender
            .send(DesktopAudioMessage::Padding(samples))
            .map_err(|_| PlaybackStopped)?;
        if self.reserve_recording_slot() {
            self.send_reserved_recording(DesktopRecordingMessage::AudioPadding {
                pts: self.next_audio_pts,
                samples,
            });
        }
        self.next_audio_pts += samples as i64;
        Ok(true)
    }
}

impl DesktopFrameSender {
    fn reserve_recording_slot(&mut self) -> bool {
        let active = self
            .recording_active
            .as_ref()
            .is_some_and(|active| active.load(Ordering::Acquire));
        if !active {
            self.recording_sender = None;
            return false;
        }
        if self.recording_sender.is_none() {
            return false;
        }
        let Some(queue_depth) = &self.recording_queue_depth else {
            return false;
        };

        if queue_depth
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |depth| {
                (depth < RECORDING_CHANNEL_CAPACITY).then_some(depth + 1)
            })
            .is_err()
        {
            self.report_recording_overload();
            return false;
        }
        true
    }

    fn release_recording_slot(&self) {
        if let Some(queue_depth) = &self.recording_queue_depth {
            queue_depth.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn send_reserved_recording(&mut self, message: DesktopRecordingMessage) {
        let Some(sender) = &self.recording_sender else {
            self.release_recording_slot();
            return;
        };
        match sender.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.release_recording_slot();
                self.report_recording_overload();
            }
            Err(TrySendError::Disconnected(_)) => {
                self.release_recording_slot();
                self.recording_sender = None;
                if self
                    .recording_active
                    .as_ref()
                    .is_some_and(|active| active.swap(false, Ordering::AcqRel))
                {
                    log::error!(channel = self.channel_id; "Recording disabled: desktop recording worker stopped unexpectedly");
                }
            }
        }
    }

    fn report_recording_overload(&mut self) {
        self.recording_dropped_messages = self.recording_dropped_messages.saturating_add(1);
        let now = Instant::now();
        let should_log = self.recording_last_overload_log.is_none_or(|last| {
            now.saturating_duration_since(last) >= RECORDING_OVERLOAD_LOG_INTERVAL
        });
        if should_log {
            log::warn!(channel = self.channel_id; "Desktop recording encoder is behind; dropped {} recording frame(s) without blocking playback", self.recording_dropped_messages);
            self.recording_dropped_messages = 0;
            self.recording_last_overload_log = Some(now);
        }
    }
}

fn take_audio_buffer(pool: &Mutex<Vec<Vec<f32>>>, capacity: usize) -> Vec<f32> {
    let mut samples = pool
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pop()
        .unwrap_or_default();
    samples.clear();
    if samples.capacity() < capacity {
        samples.reserve(capacity - samples.capacity());
    }
    samples
}

fn return_audio_buffer(pool: &Mutex<Vec<Vec<f32>>>, mut samples: Vec<f32>) {
    samples.clear();
    if samples.capacity() > AUDIO_BUFFER_MAX_RETAINED_CAPACITY {
        return;
    }

    let mut pool = pool.lock().unwrap_or_else(PoisonError::into_inner);
    if pool.len() < AUDIO_BUFFER_POOL_CAPACITY {
        pool.push(samples);
    }
}

impl DesktopRenderer {
    fn window(&self) -> &DesktopWindowHandle {
        self.window
            .as_ref()
            .expect("desktop window handle must exist while rendering")
    }

    fn open(cfg: &OutputConfig) -> Result<Self> {
        let window = prepare_desktop_window(cfg.width, cfg.height, cfg.desktop_fullscreen)?;
        let audio = DesktopAudio::open(cfg.sample_rate)?;
        let device_buffer_samples = audio.device_buffer_samples();
        let logo = cfg
            .logo
            .as_ref()
            .map(|logo| create_desktop_logo(logo, cfg.width, cfg.height))
            .transpose()?;

        Ok(Self {
            window: Some(window),
            audio,
            audio_effects_control: cfg.audio_effects.clone(),
            video_queue: VecDeque::with_capacity(VIDEO_CHANNEL_CAPACITY),
            pending_audio: VecDeque::new(),
            audio_buffer_pool: Arc::new(Mutex::new(Vec::with_capacity(AUDIO_BUFFER_POOL_CAPACITY))),
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
                Ok(message) => {
                    received = true;
                    let samples = match &message {
                        DesktopAudioMessage::Samples {
                            samples_per_channel,
                            ..
                        } => *samples_per_channel as u64,
                        DesktopAudioMessage::Padding(samples) => *samples,
                    };
                    self.pending_audio.push_back(message);
                    self.pending_audio_samples = self.pending_audio_samples.saturating_add(samples);
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
        self.recycle_pending_audio();
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
        result
    }

    fn flush_pending_audio(&mut self) -> Result<()> {
        while self.queued_audio_samples() < self.max_queue_samples() {
            // Finish the current gap before dequeuing any subsequent audio.
            if self.pending_silence_samples > 0 {
                let samples = self.pending_silence_samples.min(1_024) as usize;
                let mut silence =
                    take_audio_buffer(&self.audio_buffer_pool, samples * AUDIO_CHANNELS);
                silence.resize(samples * AUDIO_CHANNELS, 0.0);
                self.audio.queue(&silence)?;
                self.recycle_audio_buffer(silence);
                self.pending_silence_samples -= samples as u64;
                self.submitted_audio_samples =
                    self.submitted_audio_samples.saturating_add(samples as u64);
                continue;
            }
            match self.pending_audio.pop_front() {
                Some(DesktopAudioMessage::Samples {
                    samples,
                    samples_per_channel,
                }) => {
                    self.audio.queue(&samples)?;
                    self.recycle_audio_buffer(samples);
                    self.pending_audio_samples = self
                        .pending_audio_samples
                        .saturating_sub(samples_per_channel as u64);
                    self.submitted_audio_samples = self
                        .submitted_audio_samples
                        .saturating_add(samples_per_channel as u64);
                }
                Some(DesktopAudioMessage::Padding(samples)) => {
                    self.pending_audio_samples = self.pending_audio_samples.saturating_sub(samples);
                    self.apply_audio_padding(samples);
                }
                None => break,
            }
        }
        self.start_audio_if_ready(false);
        Ok(())
    }

    fn recycle_pending_audio(&mut self) {
        while let Some(message) = self.pending_audio.pop_front() {
            if let DesktopAudioMessage::Samples { samples, .. } = message {
                self.recycle_audio_buffer(samples);
            }
        }
    }

    fn recycle_audio_buffer(&self, samples: Vec<f32>) {
        return_audio_buffer(&self.audio_buffer_pool, samples);
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
        if starved && !self.video_finished && !reached_video_end {
            // A decoder is intentionally interrupted during shutdown. Wait
            // for a sustained underflow before reporting it, so the final
            // scheduler tick before a window closes is not noisy.
            let starvation_started = self.last_starvation_report.get_or_insert(now);
            if now.duration_since(*starvation_started) >= Duration::from_secs(1) {
                log::debug!(
                    "desktop video queue starved: expected pts {expected_video_pts}, last rendered \
                     pts {last_video_pts}, queued frames {}",
                    self.video_queue.len()
                );
                self.last_starvation_report = Some(now);
            }
        } else {
            self.last_starvation_report = None;
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
        if !thread::is_running() {
            pump_desktop_window_events();
        }
        let actions = self.window().take_actions();
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
                    if !self.window().fullscreen() && width > 0 && height > 0 {
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
            self.window().request_size(target.0, target.1);
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
        self.window().set_frame(WindowFrame {
            video: self.last_video.clone(),
            subtitle,
            logo,
            volume: self.audio_effects_control.volume(),
            volume_overlay,
            help,
        });
    }

    fn help_bitmap(&mut self) -> Option<RgbaBitmap> {
        if !self.help_visible {
            return None;
        }
        if self.help_bitmap.is_none() {
            let (size, large) = self.window().size_and_large_subtitles();
            self.help_bitmap = create_help_bitmap(size.0, large)
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
                let (size, large) = self.window().size_and_large_subtitles();
                create_subtitle_bitmap(&text, size.0, large)
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
        if let Some(handle) = self.window.take() {
            close_desktop_window(handle);
        }
    }
}

struct DesktopWindow {
    event_loop: EventLoop<()>,
    app: Option<DesktopWindowApp>,
}

#[derive(Clone)]
struct DesktopWindowHandle {
    window: Arc<Window>,
    shared: Arc<Mutex<DesktopWindowShared>>,
}

struct DesktopWindowShared {
    actions: Vec<WindowAction>,
    frame: Option<WindowFrame>,
    size: (u32, u32),
    fullscreen: bool,
    maximized: bool,
    requested_size: Option<(u32, u32)>,
}

impl DesktopWindowShared {
    fn update_window_state(&mut self, size: (u32, u32), maximized: bool) {
        self.size = size;
        self.maximized = maximized;
    }
}

struct DesktopWindowApp {
    window: Arc<Window>,
    renderer: WindowRenderer,
    shared: Arc<Mutex<DesktopWindowShared>>,
    size: (u32, u32),
    occluded: bool,
    last_primary_click: Option<Instant>,
}

struct DesktopWindowCreator {
    width: u32,
    height: u32,
    fullscreen: bool,
    result: Option<Result<DesktopWindowApp>>,
}

struct DesktopWindowIdle;

fn prepare_desktop_window(
    width: u32,
    height: u32,
    fullscreen: bool,
) -> Result<DesktopWindowHandle> {
    if thread::is_running() {
        thread::call(move || prepare_desktop_window_on_current_thread(width, height, fullscreen))?
    } else {
        prepare_desktop_window_on_current_thread(width, height, fullscreen)
    }
}

fn prepare_desktop_window_on_current_thread(
    width: u32,
    height: u32,
    fullscreen: bool,
) -> Result<DesktopWindowHandle> {
    DESKTOP_WINDOW.with(|window| {
        let mut window = window.borrow_mut();
        if let Some(window) = window.as_mut() {
            window.reconfigure(width, height, fullscreen)?;
            Ok(window.handle())
        } else {
            *window = Some(DesktopWindow::open(width, height, fullscreen)?);
            Ok(window
                .as_ref()
                .expect("desktop window was just initialized")
                .handle())
        }
    })
}

pub(super) fn pump_desktop_window_events() {
    DESKTOP_WINDOW.with(|window| {
        if let Some(window) = window.borrow_mut().as_mut() {
            window.pump_events();
        }
    });
}

/// Releases Winit/WGPU resources while their supporting thread-local state is
/// still alive. Leaving this to TLS destruction can make WGPU access a TLS
/// value that Rust has already torn down during process exit.
#[cfg(feature = "tokio")]
pub(super) fn release_desktop_window() {
    let _ = DESKTOP_WINDOW.try_with(|window| {
        drop(window.borrow_mut().take());
    });
}

fn close_desktop_window(handle: DesktopWindowHandle) {
    let close = move || {
        DESKTOP_WINDOW.with(|window| {
            if let Some(window) = window.borrow_mut().as_mut() {
                window.hide();
            }
        });
        // DesktopRenderer also owns a Window reference. Release it in this
        // closure so the final native Window drop always happens on the host
        // thread together with Winit and WGPU state.
        drop(handle);
    };
    if thread::is_running() {
        // Renderer teardown can originate from the playout worker while the
        // host thread is still dispatching window events. Queue the teardown,
        // but do not make the playout worker wait for the host thread.
        if let Err(error) = thread::spawn(close) {
            log::warn!("failed to schedule desktop window close: {error}");
        }
    } else {
        close();
    }
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

fn create_desktop_window_app(
    event_loop: &ActiveEventLoop,
    width: u32,
    height: u32,
    fullscreen: bool,
) -> Result<DesktopWindowApp> {
    let attributes = Window::default_attributes()
        .with_title(DESKTOP_WINDOW_TITLE)
        .with_window_icon(Some(desktop_window_icon()?))
        .with_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
        .with_resizable(true)
        .with_visible(false)
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
    let window = Arc::new(
        event_loop
            .create_window(attributes)
            .context("creating desktop window")?,
    );
    let size = window.inner_size();
    let shared = Arc::new(Mutex::new(DesktopWindowShared {
        actions: Vec::new(),
        frame: Some(WindowFrame::default()),
        size: (size.width, size.height),
        fullscreen,
        maximized: false,
        requested_size: None,
    }));
    let mut renderer = WindowRenderer::new(
        Arc::clone(&window),
        event_loop.owned_display_handle(),
        width,
        height,
    )?;
    renderer.resize_surface(size.width, size.height)?;

    let app = DesktopWindowApp {
        window,
        renderer,
        shared,
        size: (size.width, size.height),
        occluded: false,
        last_primary_click: None,
    };
    app.window.set_visible(true);
    Ok(app)
}

impl DesktopWindow {
    fn open(width: u32, height: u32, fullscreen: bool) -> Result<Self> {
        let mut event_loop_builder = EventLoop::<()>::builder();
        let event_loop = event_loop_builder
            .build()
            .context("creating desktop window event loop")?;
        let mut window = Self {
            event_loop,
            app: None,
        };
        window.create_app(width, height, fullscreen)?;
        Ok(window)
    }

    fn create_app(&mut self, width: u32, height: u32, fullscreen: bool) -> Result<()> {
        let mut creator = DesktopWindowCreator {
            width,
            height,
            fullscreen,
            result: None,
        };
        for _ in 0..3 {
            let _ = self
                .event_loop
                .pump_app_events(Some(Duration::from_millis(10)), &mut creator);
            if creator.result.is_some() {
                break;
            }
        }
        let app = creator
            .result
            .context("desktop event loop did not create a window")??;
        app.window.request_redraw();
        self.app = Some(app);
        Ok(())
    }

    fn reconfigure(&mut self, width: u32, height: u32, fullscreen: bool) -> Result<()> {
        if self.app.is_none() {
            return self.create_app(width, height, fullscreen);
        }
        let app = self
            .app
            .as_mut()
            .expect("desktop window app was checked above");
        {
            let mut shared = app.shared.lock().unwrap_or_else(PoisonError::into_inner);
            shared.frame = Some(WindowFrame::default());
            shared.actions.clear();
            shared.fullscreen = fullscreen;
            shared.maximized = false;
            shared.requested_size = None;
        }
        app.occluded = false;
        app.renderer.reset_frame_cache();
        app.window.set_fullscreen(
            fullscreen.then(|| Fullscreen::Borderless(app.window.current_monitor())),
        );

        if width > 0 && height > 0 {
            app.renderer.resize_buffer(width, height)?;
        }

        if !fullscreen && width > 0 && height > 0 {
            app.size = (width, height);
            app.shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .size = (width, height);
            app.renderer.resize_surface(width, height)?;
            let _ = app
                .window
                .request_inner_size(PhysicalSize::new(width, height));
        }
        app.window.set_visible(true);
        app.window.request_redraw();
        Ok(())
    }

    fn hide(&mut self) {
        if let Some(mut app) = self.app.take() {
            app.shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .frame = None;
            app.renderer.release_frame_resources();
            // Wayland ignores set_visible(false), so destroy the native window
            // and renderer. The EventLoop remains available for a later session.
            drop(app);
        }
    }

    fn pump_events(&mut self) {
        if let Some(app) = self.app.as_mut() {
            let _ = self.event_loop.pump_app_events(Some(Duration::ZERO), app);
        } else {
            // Winit still needs to dispatch lifecycle events after the native
            // window has been dropped. In particular, Wayland may otherwise
            // leave the destroyed surface pending until another window opens.
            let mut idle = DesktopWindowIdle;
            let _ = self
                .event_loop
                .pump_app_events(Some(Duration::ZERO), &mut idle);
        }
    }

    fn handle(&self) -> DesktopWindowHandle {
        let app = self
            .app
            .as_ref()
            .expect("desktop window must exist before creating a handle");
        DesktopWindowHandle {
            window: Arc::clone(&app.window),
            shared: Arc::clone(&app.shared),
        }
    }
}

impl DesktopWindowHandle {
    fn take_actions(&self) -> Vec<WindowAction> {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut shared.actions)
    }

    fn set_frame(&self, frame: WindowFrame) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .frame = Some(frame);
        self.window.request_redraw();
    }

    fn fullscreen(&self) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fullscreen
    }

    fn size_and_large_subtitles(&self) -> ((u32, u32), bool) {
        let shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        (shared.size, shared.fullscreen || shared.maximized)
    }

    fn request_size(&self, width: u32, height: u32) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .requested_size = Some((width, height));
        self.window.request_redraw();
    }
}

impl DesktopWindowApp {
    fn push_action(&self, action: WindowAction) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .actions
            .push(action);
    }

    fn apply_requested_size(&self) {
        let requested_size = self
            .shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .requested_size
            .take();
        if let Some((width, height)) = requested_size {
            let _ = self
                .window
                .request_inner_size(PhysicalSize::new(width, height));
        }
    }

    fn toggle_fullscreen(&mut self) {
        let fullscreen = self.window.fullscreen().is_none();
        self.window.set_fullscreen(
            fullscreen.then(|| Fullscreen::Borderless(self.window.current_monitor())),
        );
        {
            let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
            shared.fullscreen = fullscreen;
            shared.actions.push(WindowAction::FullscreenChanged);
        }
    }
}

impl DesktopWindowCreator {
    fn create(&mut self, event_loop: &ActiveEventLoop) {
        if self.result.is_none() {
            self.result = Some(create_desktop_window_app(
                event_loop,
                self.width,
                self.height,
                self.fullscreen,
            ));
        }
    }
}

impl ApplicationHandler for DesktopWindowCreator {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.create(event_loop);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.create(event_loop);
    }
}

impl ApplicationHandler for DesktopWindowIdle {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
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

        self.apply_requested_size();

        match event {
            WindowEvent::CloseRequested => self.push_action(WindowAction::Stop),
            WindowEvent::Resized(size) => {
                self.size = (size.width, size.height);
                self.shared
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .update_window_state(self.size, self.window.is_maximized());
                if let Err(error) = self.renderer.resize_surface(size.width, size.height) {
                    log::warn!("desktop renderer resize failed: {error}");
                }
                self.push_action(WindowAction::Resize(size.width, size.height));
                self.window.request_redraw();
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    self.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let frame = self
                    .shared
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .frame
                    .clone();
                if !self.occluded
                    && let Some(frame) = &frame
                    && let Err(error) = benchmark::measure_success(Stage::DesktopPresent, || {
                        self.renderer.render(frame, self.size)
                    })
                {
                    log::warn!("desktop renderer failed: {error}");
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => self.push_action(WindowAction::Stop),
                    PhysicalKey::Code(KeyCode::KeyF) if !event.repeat => {
                        self.toggle_fullscreen();
                    }
                    PhysicalKey::Code(KeyCode::KeyS) if !event.repeat => {
                        self.push_action(WindowAction::ToggleSubtitles);
                    }
                    PhysicalKey::Code(KeyCode::KeyE) if !event.repeat => {
                        self.push_action(WindowAction::Control(DesktopControlCommand::Back));
                    }
                    PhysicalKey::Code(KeyCode::KeyT) if !event.repeat => {
                        self.push_action(WindowAction::Control(DesktopControlCommand::Next));
                    }
                    PhysicalKey::Code(KeyCode::KeyR) if !event.repeat => {
                        self.push_action(WindowAction::Control(DesktopControlCommand::Reset));
                    }
                    PhysicalKey::Code(KeyCode::KeyH) if !event.repeat => {
                        self.push_action(WindowAction::ToggleHelp);
                    }
                    PhysicalKey::Code(KeyCode::ArrowLeft) => {
                        self.push_action(WindowAction::AdjustVolume(-DESKTOP_VOLUME_STEP));
                    }
                    PhysicalKey::Code(KeyCode::ArrowRight) => {
                        self.push_action(WindowAction::AdjustVolume(DESKTOP_VOLUME_STEP));
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

    fn headless_audio_renderer() -> DesktopRenderer {
        DesktopRenderer {
            window: None,
            audio: DesktopAudio::for_test(),
            audio_effects_control: AudioEffectsControl::default(),
            video_queue: VecDeque::new(),
            pending_audio: VecDeque::new(),
            audio_buffer_pool: Arc::new(Mutex::new(Vec::new())),
            pending_audio_samples: 0,
            pending_silence_samples: 0,
            submitted_audio_samples: 0,
            audio_started: false,
            sample_rate: 48_000,
            device_buffer_samples: 1_024,
            audio_clock: AudioMasterClock::new(48_000, 1_024),
            video_time_base: Rational(1, 25),
            video_end_pts: None,
            video_decoded: false,
            video_finished: false,
            last_rendered_video_pts: None,
            last_video_present: None,
            last_starvation_report: None,
            fps: 25,
            subtitles_enabled: false,
            subtitles: Vec::new(),
            active_subtitle_text: None,
            subtitle_bitmap: None,
            logo: None,
            current_logo_opacity: 0.0,
            aspect_width: 16,
            aspect_height: 16,
            last_window_size: (16, 16),
            pending_aspect_resize: None,
            volume_overlay_until: None,
            last_video: None,
            frame_converter: DesktopFrameConverter::default(),
            desktop_control_callback: None,
            help_visible: false,
            help_bitmap: None,
        }
    }

    #[test]
    fn regression_live_padding_precedes_resumed_audio() {
        let mut renderer = headless_audio_renderer();
        let (audio_tx, audio_rx) = sync_channel(2);
        // No wall-clock underflow has covered this gap. Both messages arrive
        // before one scheduler tick, as can happen when buffered live audio resumes.
        audio_tx.send(DesktopAudioMessage::Padding(4_800)).unwrap();
        audio_tx
            .send(DesktopAudioMessage::Samples {
                samples: vec![0.25; 2_048],
                samples_per_channel: 1_024,
            })
            .unwrap();
        renderer.drain_audio_messages(&audio_rx);
        renderer.flush_pending_audio().unwrap();
        let samples = renderer.audio.samples_for_test();
        assert_eq!(samples.len(), (4_800 + 1_024) * 2);
        assert!(
            samples[..4_800 * 2].iter().all(|sample| *sample == 0.0),
            "silence must precede resumed audio in the actual device queue"
        );
        assert!(samples[4_800 * 2..].iter().all(|sample| *sample == 0.25));
    }

    #[test]
    fn regression_live_padding_reaches_recording() {
        let (video_sender, _video_rx) = sync_channel(2);
        let (audio_sender, audio_rx) = sync_channel(2);
        let (control_sender, _control_rx) = sync_channel(2);
        let (discontinuity_sender, _discontinuity_rx) = sync_channel(1);
        let (recording_sender, recording_rx) = sync_channel(256);
        let mut sender = DesktopFrameSender {
            next_audio_pts: 480_000,
            video_sender,
            audio_sender,
            control_sender,
            discontinuity_sender,
            audio_effects: Arc::new(Mutex::new(AudioEffectChain::new(
                AudioEffectsControl::default(),
                48_000,
            ))),
            audio_buffer_pool: Arc::new(Mutex::new(Vec::new())),
            audio_level_meter: AudioLevelMeter::new(48_000, None),
            loudness_meter: LoudnessMeter::new(48_000, Default::default()),
            current_logo_opacity: 0.0,
            recording_sender: Some(recording_sender),
            recording_active: Some(Arc::new(AtomicBool::new(true))),
            recording_queue_depth: Some(Arc::new(AtomicUsize::new(0))),
            recording_logo: None,
            recording_dropped_messages: 0,
            recording_last_overload_log: None,
            channel_id: 0,
        };
        assert!(sender.pad_audio(144_000).unwrap());
        assert!(matches!(
            audio_rx.try_recv(),
            Ok(DesktopAudioMessage::Padding(144_000))
        ));
        let samples: usize = recording_rx
            .try_iter()
            .map(|message| match message {
                DesktopRecordingMessage::Audio(frame) => frame.samples(),
                DesktopRecordingMessage::AudioPadding { pts, samples } => {
                    assert_eq!(pts, 480_000);
                    let mut expected_pts = pts;
                    recording_silence(pts, samples, 48_000)
                        .map(|frame| {
                            assert_eq!(frame.pts(), Some(expected_pts));
                            expected_pts += frame.samples() as i64;
                            for channel in 0..2 {
                                assert!(
                                    frame
                                        .plane::<f32>(channel)
                                        .iter()
                                        .all(|sample| *sample == 0.0)
                                );
                            }
                            frame.samples()
                        })
                        .sum()
                }
                _ => 0,
            })
            .sum();
        assert_eq!(
            samples, 144_000,
            "recording must contain the same three seconds of silence"
        );
    }

    #[test]
    fn audio_padding_preserves_order_across_multiple_queue_drains() {
        let mut renderer = headless_audio_renderer();
        let (tx, rx) = sync_channel(3);
        tx.send(DesktopAudioMessage::Samples {
            samples: vec![0.125; 2_048],
            samples_per_channel: 1_024,
        })
        .unwrap();
        tx.send(DesktopAudioMessage::Padding(48_000)).unwrap();
        tx.send(DesktopAudioMessage::Samples {
            samples: vec![0.25; 2_048],
            samples_per_channel: 1_024,
        })
        .unwrap();
        let mut played = Vec::new();
        for _ in 0..10 {
            renderer.drain_audio_messages(&rx);
            renderer.flush_pending_audio().unwrap();
            played.extend(renderer.audio.samples_for_test());
            // Simulate the audio device consuming its currently queued samples.
            renderer.audio.clear();
        }
        assert_eq!(played.len(), (1_024 + 48_000 + 1_024) * 2);
        assert!(played[..2_048].iter().all(|sample| *sample == 0.125));
        assert!(played[2_048..98_048].iter().all(|sample| *sample == 0.0));
        assert!(played[98_048..].iter().all(|sample| *sample == 0.25));
        assert_eq!(renderer.pending_silence_samples, 0);
        assert_eq!(renderer.pending_audio_samples, 0);
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
    fn audio_buffer_pool_reuses_normal_buffers_and_drops_oversized_ones() {
        let pool = Mutex::new(Vec::new());
        let reusable = Vec::<f32>::with_capacity(AUDIO_BUFFER_MAX_RETAINED_CAPACITY);
        return_audio_buffer(&pool, reusable);
        assert_eq!(pool.lock().expect("audio pool lock").len(), 1);

        let oversized = Vec::<f32>::with_capacity(AUDIO_BUFFER_MAX_RETAINED_CAPACITY + 1);
        return_audio_buffer(&pool, oversized);
        assert_eq!(pool.lock().expect("audio pool lock").len(), 1);
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
            pixels: Arc::new(video::RecyclableBuffer::unpooled(vec![1, 2, 3, 4])),
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
