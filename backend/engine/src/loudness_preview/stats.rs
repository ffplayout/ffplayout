use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread::{self, JoinHandle},
};

use anyhow::Result;
use ffmpeg_next::{Rational, Rescale, frame};

use crate::{
    LiveLoudnessMetrics,
    analysis::loudness::{LoudnessAnalyzer, LoudnessMetrics},
    compositor::text::TextOverlay,
    utils::{
        config::{RgbaColor, TextBackgroundConfig, TextConfig, TextPosition},
        ffmpeg::make_video_frame_writable,
    },
};

const MAX_PENDING_SNAPSHOTS: usize = 64;
const UPDATES_PER_SECOND: usize = 4;

struct RenderRequest {
    text: String,
    width: u32,
    height: u32,
    time_base: Rational,
}

struct OverlayRenderer {
    sender: Option<SyncSender<RenderRequest>>,
    receiver: Receiver<Result<Option<TextOverlay>>>,
    worker: Option<JoinHandle<()>>,
}

impl OverlayRenderer {
    fn new() -> Result<Self> {
        let (sender, requests) = mpsc::sync_channel(1);
        let (results, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("ffplayout-audio-stats".to_owned())
            .spawn(move || {
                while let Ok(request) = requests.recv() {
                    let overlay = render_overlay(request);

                    if results.send(overlay).is_err() {
                        break;
                    }
                }
            })?;

        Ok(Self {
            sender: Some(sender),
            receiver,
            worker: Some(worker),
        })
    }

    fn submit(&self, request: RenderRequest) -> bool {
        match self.sender.as_ref().map(|sender| sender.try_send(request)) {
            Some(Ok(())) => true,
            Some(Err(TrySendError::Full(_))) | None => false,
            Some(Err(TrySendError::Disconnected(_))) => false,
        }
    }
}

impl Drop for OverlayRenderer {
    fn drop(&mut self) {
        self.sender.take();
        // Release a worker waiting to return its final bitmap before joining it.
        while self.receiver.try_recv().is_ok() {}

        if let Some(worker) = self.worker.take() {
            // The receiver must be closed before joining: a render may still be
            // in flight and would otherwise block on a full result channel.
            let (_sender, empty_receiver) = mpsc::sync_channel(1);
            let receiver = std::mem::replace(&mut self.receiver, empty_receiver);
            drop(receiver);
            let _ = worker.join();
        }
    }
}

fn render_overlay(request: RenderRequest) -> Result<Option<TextOverlay>> {
    let config = TextConfig {
        text: Some(request.text),
        font_family: Some("DejaVu Sans Mono".to_owned()),
        font_size: 20.0,
        position_x: TextPosition::Pixels(18),
        position_y: TextPosition::Pixels(88),
        background: Some(TextBackgroundConfig {
            color: RgbaColor {
                r: 0,
                g: 0,
                b: 0,
                a: 210,
            },
            padding: 12,
        }),
        ..TextConfig::default()
    };
    let fps = (f64::from(request.time_base.denominator())
        / f64::from(request.time_base.numerator()))
    .round()
    .max(1.0) as u32;

    TextOverlay::load(&config, "", request.width, request.height, fps, 0, 0, None)
}

#[derive(Default)]
struct Levels {
    squares: f64,
    peak: f32,
    samples: usize,
}

impl Levels {
    fn observe(&mut self, audio: &frame::Audio) {
        for channel in 0..2 {
            for &sample in audio.plane::<f32>(channel) {
                let sample = if sample.is_finite() { sample } else { 0.0 };
                self.squares += f64::from(sample).powi(2);
                self.peak = self.peak.max(sample.abs());
                self.samples += 1;
            }
        }
    }

    fn rms(&self) -> f64 {
        10.0 * (self.squares / self.samples.max(1) as f64)
            .max(1e-10)
            .log10()
    }

    fn peak(&self) -> f64 {
        20.0 * f64::from(self.peak).max(1e-5).log10()
    }
}

/// Measurements follow emitted audio; snapshots are selected using video PTS,
/// never decoder wall time. Both snapshot and text caches remain bounded.
pub(crate) struct AudioStatsOverlay {
    input: LoudnessAnalyzer,
    output: LoudnessAnalyzer,
    input_loudness: LoudnessMetrics,
    input_levels: Levels,
    output_levels: Levels,
    samples: usize,
    sample_rate: u32,
    compressor_db: f64,
    limiter_db: f64,
    pending: VecDeque<(i64, String)>,
    text: String,
    overlay: Option<TextOverlay>,
    dimensions: Option<(u32, u32)>,
    renderer: OverlayRenderer,
    render_dirty: bool,
}

impl AudioStatsOverlay {
    pub(crate) fn new(sample_rate: u32) -> Result<Self> {
        Ok(Self {
            input: LoudnessAnalyzer::new_display_meter(sample_rate)?,
            output: LoudnessAnalyzer::new_display_meter(sample_rate)?,
            input_loudness: LoudnessMetrics::default(),
            input_levels: Levels::default(),
            output_levels: Levels::default(),
            samples: 0,
            sample_rate,
            compressor_db: 0.0,
            limiter_db: 0.0,
            pending: VecDeque::new(),
            text: "AUDIO STATS | waiting for audio\nIN / OUT: measuring...".to_owned(),
            overlay: None,
            dimensions: None,
            renderer: OverlayRenderer::new()?,
            render_dirty: true,
        })
    }

    pub(crate) fn observe_input(&mut self, audio: &frame::Audio) {
        self.input_loudness = self.input.process_frame(audio);
        self.input_levels.observe(audio);
    }

    pub(crate) fn observe_output(
        &mut self,
        audio: &frame::Audio,
        metrics: LiveLoudnessMetrics,
        compressor_db: Option<f64>,
        pause: Option<&str>,
    ) {
        let output_loudness = self.output.process_frame(audio);
        self.output_levels.observe(audio);
        self.samples += audio.samples();
        self.compressor_db = self.compressor_db.max(compressor_db.unwrap_or_default());
        self.limiter_db = self.limiter_db.max(metrics.limiter_gain_reduction_db);

        if self.samples < self.sample_rate as usize / UPDATES_PER_SECOND {
            return;
        }

        let position = audio.pts().unwrap_or_default() + audio.samples() as i64;
        let seconds = position as f64 / f64::from(self.sample_rate);
        let compressor = compressor_db.map_or_else(
            || "n/a".to_owned(),
            |_| format!("-{:4.1} dB", self.compressor_db),
        );
        let text = format!(
            "AUDIO STATS | {seconds:6.1} s | {}\n\
             Loudness       IN         OUT\n\
             M (400 ms)  {:>7}     {:>7} LUFS\n\
             S (3 s)     {:>7}     {:>7} LUFS\n\
             RMS IN  [{}] {:6.1} dBFS\n\
             RMS OUT [{}] {:6.1} dBFS\n\
             Peak        {:7.1}     {:7.1} dBFS\n\
             AGC {:+5.1} dB | COMP {compressor}\n\
             LIMITER -{:4.1} dB | Net {:+5.1} dB\n\
             OUT: before player volume",
            pause.unwrap_or("RIDER"),
            loudness(self.input_loudness.momentary_lufs),
            loudness(output_loudness.momentary_lufs),
            loudness(self.input_loudness.short_term_lufs),
            loudness(output_loudness.short_term_lufs),
            meter(self.input_levels.rms()),
            self.input_levels.rms(),
            meter(self.output_levels.rms()),
            self.output_levels.rms(),
            self.input_levels.peak(),
            self.output_levels.peak(),
            metrics.rider_gain_db,
            self.limiter_db,
            self.output_levels.rms() - self.input_levels.rms(),
        );
        self.pending.push_back((position, text));

        while self.pending.len() > MAX_PENDING_SNAPSHOTS {
            self.pending.pop_front();
        }

        self.input_levels = Levels::default();
        self.output_levels = Levels::default();
        self.samples = 0;
        self.compressor_db = 0.0;
        self.limiter_db = 0.0;
    }

    fn select_text(&mut self, position: i64) -> bool {
        let mut changed = false;

        while self
            .pending
            .front()
            .is_some_and(|&(pts, _)| pts <= position)
        {
            if let Some((_, text)) = self.pending.pop_front() {
                self.text = text;
                changed = true;
            }
        }

        changed
    }

    pub(crate) fn blend(&mut self, video: &mut frame::Video, time_base: Rational) -> Result<()> {
        let pts = video.pts().unwrap_or_default();
        let sample_position = pts.rescale(time_base, Rational(1, self.sample_rate as i32));
        let changed = self.select_text(sample_position);
        let dimensions = (video.width(), video.height());

        while let Ok(overlay) = self.renderer.receiver.try_recv() {
            self.overlay = overlay?;
        }

        self.render_dirty |= changed || self.dimensions != Some(dimensions);

        if self.render_dirty
            && self.renderer.submit(RenderRequest {
                text: self.text.clone(),
                width: dimensions.0,
                height: dimensions.1,
                time_base,
            })
        {
            self.render_dirty = false;
            self.dimensions = Some(dimensions);
        }

        if let Some(overlay) = self.overlay.as_mut() {
            make_video_frame_writable(video)?;
            overlay.blend(video, pts, pts);
        }

        Ok(())
    }
}

fn meter(dbfs: f64) -> String {
    let filled = (((dbfs + 60.0) / 60.0).clamp(0.0, 1.0) * 24.0).round() as usize;

    format!("{}{}", "#".repeat(filled), ".".repeat(24 - filled))
}

fn loudness(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite())
        .map_or_else(|| "--".to_owned(), |value| format!("{value:.1}"))
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        ChannelLayout,
        format::{Sample, sample::Type},
    };

    use super::*;

    fn audio(value: f32) -> frame::Audio {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 4800, ChannelLayout::STEREO);
        audio.set_pts(Some(0));
        audio.set_rate(48000);
        audio.plane_mut::<f32>(0).fill(value);
        audio.plane_mut::<f32>(1).fill(-value);
        audio
    }

    #[test]
    fn stereo_measurement_does_not_cancel_opposite_phase_channels() {
        let mut levels = Levels::default();
        levels.observe(&audio(0.1));

        assert!((levels.rms() + 20.0).abs() < 0.001);
        assert!((levels.peak() + 20.0).abs() < 0.001);
    }

    #[test]
    fn snapshots_wait_for_playback_pts_and_show_actual_input_output_gain() {
        let mut stats = AudioStatsOverlay::new(48000).unwrap();
        for index in 0..3 {
            let mut input = audio(0.1);
            let mut output = audio(0.05);
            input.set_pts(Some(index * 4800));
            output.set_pts(Some(index * 4800));
            stats.observe_input(&input);
            stats.observe_output(
                &output,
                LiveLoudnessMetrics::default(),
                Some(6.0),
                Some("SIGNAL"),
            );
        }
        assert!(!stats.select_text(14399));
        assert!(stats.text.contains("waiting for audio"));
        assert!(stats.select_text(14400));
        assert!(stats.text.contains("COMP - 6.0 dB"));
        assert!(stats.text.contains("Net  -6.0 dB"));
        assert!(stats.pending.is_empty());
    }

    #[test]
    fn audio_without_video_keeps_snapshot_memory_bounded() {
        let mut stats = AudioStatsOverlay::new(48000).unwrap();

        for index in 0..300 {
            let mut frame = audio(0.1);
            frame.set_pts(Some(index * 4800));
            stats.observe_input(&frame);
            stats.observe_output(&frame, LiveLoudnessMetrics::default(), None, None);
        }

        assert_eq!(stats.pending.len(), MAX_PENDING_SNAPSHOTS);
        assert!(stats.select_text(1440000));
        assert!(stats.text.contains("COMP n/a"));
        assert!(stats.pending.is_empty());
    }
    #[test]
    fn rendered_statistics_are_visible_and_preserve_shared_video_buffers() {
        use ffmpeg_next::format::Pixel;

        use crate::utils::ffmpeg::reference_video_frame;

        ffmpeg_next::init().unwrap();
        let mut stats = AudioStatsOverlay::new(48000).unwrap();
        for index in 0..3 {
            let mut input = audio(0.1);
            let mut output = audio(0.05);
            input.set_pts(Some(index * 4800));
            output.set_pts(Some(index * 4800));
            stats.observe_input(&input);
            stats.observe_output(
                &output,
                LiveLoudnessMetrics {
                    rider_gain_db: 2.0,
                    limiter_gain_reduction_db: 0.5,
                    ..LiveLoudnessMetrics::default()
                },
                Some(7.5),
                Some("PAUSE / HOLD"),
            );
        }
        let mut original = frame::Video::new(Pixel::YUV420P, 1280, 720);
        original.data_mut(0).fill(16);
        original.data_mut(1).fill(128);
        original.data_mut(2).fill(128);
        original.set_pts(Some(8));
        let mut video = reference_video_frame(&original).unwrap();
        stats.blend(&mut video, Rational(1, 25)).unwrap();
        stats.overlay = stats
            .renderer
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        stats.blend(&mut video, Rational(1, 25)).unwrap();

        assert!(stats.text.contains("PAUSE / HOLD"));
        assert!(stats.overlay.is_some());
        assert!(video.data(0).iter().any(|&pixel| pixel > 32));
        assert!(original.data(0).iter().all(|&pixel| pixel == 16));
        assert_eq!(video.pts(), Some(8));
    }
    #[test]
    fn full_render_queue_drops_updates_without_waiting_for_the_renderer() {
        let (sender, _requests) = mpsc::sync_channel(1);
        let (_results, receiver) = mpsc::sync_channel(1);
        let renderer = OverlayRenderer {
            sender: Some(sender),
            receiver,
            worker: None,
        };
        let request = || RenderRequest {
            text: "stats".to_owned(),
            width: 1280,
            height: 720,
            time_base: Rational(1, 25),
        };

        assert!(renderer.submit(request()));
        assert!(!renderer.submit(request()));
        assert!(!renderer.submit(request()));
    }
}
