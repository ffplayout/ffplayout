use std::{
    collections::VecDeque,
    mem::size_of,
    sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use ffmpeg_next::{Rational, Rescale, frame};
use sdl2::{
    Sdl,
    audio::{AudioQueue, AudioSpecDesired},
    event::Event,
    keyboard::Keycode,
    pixels::{Color, PixelFormatEnum},
    render::{Canvas, Texture},
    video::Window,
};

use super::{FrameOutput, PlaybackStopped};
use crate::utils::config::OutputConfig;

const AUDIO_CHANNELS: usize = 2;
const AUDIO_PREBUFFER_MS: u64 = 100;
const AUDIO_MAX_QUEUE_MS: u64 = 250;
const AUDIO_MAX_PENDING_MS: u64 = 500;
const VIDEO_PREBUFFER_FRAMES: usize = 3;
const VIDEO_DROP_THRESHOLD_FRAMES: i64 = 3;
const VIDEO_STARVATION_GRACE_FRAMES: i64 = 2;
const SCHEDULER_INTERVAL: Duration = Duration::from_millis(2);
const OUTPUT_CHANNEL_CAPACITY: usize = 64;

fn video_prebuffer_ready(queue_len: usize, video_finished: bool, force: bool) -> bool {
    force || video_finished || queue_len >= VIDEO_PREBUFFER_FRAMES
}

fn input_can_pause(video_queue_len: usize, video_finished: bool) -> bool {
    video_finished || video_queue_len >= VIDEO_PREBUFFER_FRAMES
}

fn video_frame_is_too_late(frame_pts: i64, expected_pts: i64, queue_len: usize) -> bool {
    queue_len > 1 && expected_pts.saturating_sub(frame_pts) > VIDEO_DROP_THRESHOLD_FRAMES
}

pub(super) struct DesktopOutput {
    renderer: DesktopRenderer,
}

enum DesktopMessage {
    ClipStarted,
    Video(frame::Video),
    Audio {
        samples: Vec<f32>,
        samples_per_channel: usize,
    },
    VideoEnd(Option<i64>),
    VideoFinished,
    ClipFinished,
}

pub(crate) struct DesktopFrameSender {
    sender: SyncSender<DesktopMessage>,
}

struct DesktopRenderer {
    // Texture must be dropped before its parent canvas.
    texture: Texture,
    canvas: Canvas<Window>,
    audio: AudioQueue<f32>,
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
    // SDL context must outlive all resources above.
    _sdl: Sdl,
}

impl DesktopOutput {
    pub(super) fn open(cfg: &OutputConfig) -> Result<Self> {
        Ok(Self {
            renderer: DesktopRenderer::open(cfg)?,
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

    pub(super) fn run_operation<T, F>(&mut self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut DesktopFrameSender) -> T + Send + 'static,
    {
        let (sender, receiver) = sync_channel(OUTPUT_CHANNEL_CAPACITY);
        let worker = thread::Builder::new()
            .name("ffplayout-decode".to_string())
            .spawn(move || {
                let mut output = DesktopFrameSender { sender };
                let _ = output.sender.send(DesktopMessage::ClipStarted);
                let result = operation(&mut output);
                let _ = output.sender.send(DesktopMessage::ClipFinished);
                result
            })
            .map_err(|error| anyhow!("failed to start decode worker: {error}"))?;

        let render_result = self.renderer.run_clip(receiver);
        let worker_result = worker
            .join()
            .map_err(|_| anyhow!("decode worker panicked"))?;
        render_result?;
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
        self.sender
            .send(DesktopMessage::Video(frame.clone()))
            .map_err(|_| PlaybackStopped.into())
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        if frame.samples() == 0 {
            return Ok(());
        }

        let left = frame.plane::<f32>(0);
        let right = frame.plane::<f32>(1);
        let mut interleaved = Vec::with_capacity(frame.samples() * AUDIO_CHANNELS);
        for (left, right) in left.iter().zip(right) {
            interleaved.push(if left.is_finite() { *left } else { 0.0 });
            interleaved.push(if right.is_finite() { *right } else { 0.0 });
        }
        self.sender
            .send(DesktopMessage::Audio {
                samples: interleaved,
                samples_per_channel: frame.samples(),
            })
            .map_err(|_| PlaybackStopped.into())
    }

    fn set_video_end(&mut self, video_end_pts: Option<i64>) -> Result<()> {
        self.sender
            .send(DesktopMessage::VideoEnd(video_end_pts))
            .map_err(|_| PlaybackStopped.into())
    }

    fn video_finished(&mut self) -> Result<()> {
        self.sender
            .send(DesktopMessage::VideoFinished)
            .map_err(|_| PlaybackStopped.into())
    }
}

impl DesktopRenderer {
    fn open(cfg: &OutputConfig) -> Result<Self> {
        // All desktop-output sessions (across restarts and channels) are
        // routed through the single persistent thread in `sdl_thread`, so SDL
        // and libdecor/GTK always see the same OS thread for their whole
        // process lifetime. That thread affinity is what libdecor's GTK
        // plugin needs; without it window creation can fail with
        // "Failed to load plugin 'libdecor-gtk.so': failed to init".
        //
        // By default SDL installs its own SIGINT/SIGTERM handlers and turns
        // them into an `Event::Quit`, which `handle_events` treats as "stop
        // the current clip" (the same as closing the window or pressing
        // Escape) rather than as a request to terminate the whole process.
        // Since the channel supervisor automatically restarts a stopped
        // channel, Ctrl-C would just restart playback instead of exiting the
        // process. Disabling SDL's signal handlers restores the normal OS
        // default (Ctrl-C terminates the process), matching the behavior of
        // the other output modes, which don't use SDL at all.
        sdl2::hint::set("SDL_NO_SIGNAL_HANDLERS", "1");
        sdl2::hint::set("SDL_VIDEO_WAYLAND_PREFER_LIBDECOR", "0");
        let sdl = sdl2::init().map_err(anyhow::Error::msg)?;
        let video = sdl.video().map_err(anyhow::Error::msg)?;
        let audio_subsystem = sdl.audio().map_err(anyhow::Error::msg)?;
        let window = video
            .window("ffplayout", cfg.width, cfg.height)
            .position_centered()
            .resizable()
            .build()
            .map_err(|error| anyhow!("{error}"))?;
        let canvas = window
            .into_canvas()
            .accelerated()
            .build()
            .map_err(|error| anyhow!("{error}"))?;
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
            audio,
            event_pump,
            video_queue: VecDeque::new(),
            pending_audio: VecDeque::new(),
            pending_audio_samples: 0,
            submitted_audio_samples: 0,
            audio_started: false,
            sample_rate: cfg.sample_rate,
            device_buffer_samples,
            audio_clock: AudioMasterClock::new(cfg.sample_rate, device_buffer_samples),
            video_time_base: cfg.video_time_base,
            video_end_pts: None,
            video_finished: false,
            last_rendered_video_pts: None,
            last_video_present: None,
            last_starvation_report: None,
            texture,
            _sdl: sdl,
        })
    }

    fn run_clip(&mut self, receiver: Receiver<DesktopMessage>) -> Result<()> {
        loop {
            self.handle_events()?;
            self.flush_pending_audio()?;
            self.render_due_video()?;

            if self.pending_audio_samples >= self.max_pending_samples()
                && input_can_pause(self.video_queue.len(), self.video_finished)
            {
                thread::sleep(SCHEDULER_INTERVAL);
                continue;
            }

            match receiver.recv_timeout(SCHEDULER_INTERVAL) {
                Ok(DesktopMessage::ClipStarted) => {
                    self.video_end_pts = None;
                    self.video_finished = false;
                    self.last_starvation_report = None;
                }
                Ok(DesktopMessage::Video(frame)) => {
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

    fn finish(mut self) -> Result<()> {
        self.flush_pending_audio()?;
        self.start_audio_if_ready(true);
        while !self.pending_audio.is_empty() || self.audio.size() > 0 {
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
            self.audio_clock.reset(Instant::now());
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
        self.canvas.present();
        Ok(())
    }

    fn render_black_frame(&mut self) {
        self.canvas.set_draw_color(Color::RGB(0, 0, 0));
        self.canvas.clear();
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
        if self.event_pump.poll_iter().any(|event| {
            matches!(
                event,
                Event::Quit { .. }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    }
            )
        }) {
            return Err(PlaybackStopped.into());
        }
        Ok(())
    }
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

    fn reset(&mut self, now: Instant) {
        self.last_consumed_samples = 0;
        self.anchor_samples = 0;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_clock_interpolates_between_device_buffer_requests() {
        let start = Instant::now();
        let mut clock = AudioMasterClock::new(48_000, 1_024);
        clock.reset(start);

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
        clock.reset(start);

        assert_eq!(clock.position(4_800, 2_752, start), 1_024);
        assert_eq!(
            clock.position(4_800, 2_752, start + Duration::from_millis(10)),
            1_504
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
}
