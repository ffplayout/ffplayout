use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
    thread,
};

use anyhow::Result;
use ffmpeg_next::{Packet, Rational, Rescale, codec, format, frame, rescale};

mod audio;
mod demux;
mod read_rate;
mod scheduler;
mod video;

use crate::{
    OutputConfig,
    benchmark::{self, Stage},
    compositor::logo::{LogoOverlay, blend_logo},
    input::diagnostics::LiveInputDiagnostics,
    output::FrameOutput,
    utils::{
        ffmpeg::{reference_audio_frame, reference_video_frame},
        pipeline_queue::{PipelineControl, PipelineQueue, QueueLimits},
    },
};

use read_rate::ReadRate;
use scheduler::coordinate;

use super::{
    ExternalAudioInput, InputPlaybackOptions, LogoFadePlan, MediaFadePlan, Timeline,
    has_valid_frame_rate, has_valid_time_base, stream_duration_us,
};

const PACKET_LIMITS: QueueLimits = QueueLimits {
    items: 256,
    bytes: 16 * 1024 * 1024,
    duration_us: 2_000_000,
};
const VIDEO_LIMITS: QueueLimits = QueueLimits {
    items: 4,
    bytes: 64 * 1024 * 1024,
    duration_us: 200_000,
};
const AUDIO_LIMITS: QueueLimits = QueueLimits {
    items: 24,
    bytes: 2 * 1024 * 1024,
    duration_us: 500_000,
};
const NO_TIMESTAMP: i64 = i64::MIN;
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(super) struct StreamSpec {
    index: usize,
    parameters: Mutex<codec::Parameters>,
    time_base: Rational,
    average_rate: Rational,
    rate: Rational,
    start_us: Option<i64>,
    end_us: Option<i64>,
    frames: i64,
    #[cfg(test)]
    decode_delay_ms: u64,
}

pub(super) fn stream_start_us(stream: &format::stream::Stream) -> Option<i64> {
    let pts = stream.start_time();
    (pts != ffmpeg_next::ffi::AV_NOPTS_VALUE && has_valid_time_base(stream.time_base()))
        .then(|| pts.rescale(stream.time_base(), rescale::TIME_BASE))
}

/// A complete ISO BMFF sample index gives the exact final DTS, including
/// B-frame reorder delay and after a seek. Sparse/keyframe-only indexes must
/// not be used as an end marker.
fn final_video_dts(input: &format::context::Input, spec: &StreamSpec) -> Option<i64> {
    if !input.format().name().split(',').any(|name| name == "mov") || spec.frames <= 0 {
        return None;
    }

    let stream = input.stream(spec.index)?;
    // SAFETY: the input and stream are alive and exclusively owned by this
    // thread. These calls inspect the already-built index; no entry is changed.
    unsafe {
        let stream = stream.as_ptr();
        let entries = ffmpeg_next::ffi::avformat_index_get_entries_count(stream);

        if i64::from(entries) != spec.frames {
            return None;
        }

        let entry = ffmpeg_next::ffi::avformat_index_get_entry(stream.cast_mut(), entries - 1);

        if entry.is_null() {
            return None;
        }

        let timestamp = (*entry).timestamp;

        (timestamp != ffmpeg_next::ffi::AV_NOPTS_VALUE).then_some(timestamp)
    }
}

impl StreamSpec {
    pub(super) fn new(stream: &format::stream::Stream) -> Self {
        Self {
            index: stream.index(),
            parameters: Mutex::new(stream.parameters().clone()),
            time_base: stream.time_base(),
            average_rate: stream.avg_frame_rate(),
            rate: stream.rate(),
            start_us: stream_start_us(stream),
            frames: stream.frames(),
            end_us: stream_duration_us(stream).map(|duration| {
                stream_start_us(stream)
                    .unwrap_or(0)
                    .saturating_add(duration)
            }),
            #[cfg(test)]
            decode_delay_ms: 0,
        }
    }

    pub(super) fn parameters(&self) -> codec::Parameters {
        self.parameters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn time_base(&self) -> Rational {
        self.time_base
    }

    pub(super) fn avg_frame_rate(&self) -> Rational {
        self.average_rate
    }

    pub(super) fn rate(&self) -> Rational {
        self.rate
    }
}

pub(super) struct Plan {
    pub(super) video: Option<StreamSpec>,
    pub(super) audio: Option<StreamSpec>,
    pub(super) has_audio: bool,
    pub(super) seek_us: i64,
    pub(super) source_origin_us: i64,
    pub(super) video_limit_pts: Option<i64>,
    pub(super) audio_limit_pts: Option<i64>,
    pub(super) video_end_pts: Option<i64>,
    pub(super) audio_end_pts: Option<i64>,
    pub(super) audio_padding_end_pts: Option<i64>,
    pub(super) video_padding_end_pts: Option<i64>,
    pub(super) logo_fade_plan: LogoFadePlan,
    pub(super) media_fade_plan: MediaFadePlan,
}

fn earlier_end(requested: Option<i64>, declared: Option<i64>) -> Option<i64> {
    match (requested, declared) {
        (Some(requested), Some(declared)) => Some(requested.min(declared)),
        (requested, declared) => requested.or(declared),
    }
}

struct Progress {
    demux_blocked_track: AtomicI64,
    video_source_us: AtomicI64,
    audio_source_us: AtomicI64,
}

impl Progress {
    fn new() -> Self {
        Self {
            demux_blocked_track: AtomicI64::new(NO_TIMESTAMP),
            video_source_us: AtomicI64::new(NO_TIMESTAMP),
            audio_source_us: AtomicI64::new(NO_TIMESTAMP),
        }
    }
}

fn load_timestamp(timestamp: &AtomicI64) -> Option<i64> {
    let value = timestamp.load(Ordering::Relaxed);
    (value != NO_TIMESTAMP).then_some(value)
}

struct VideoFrame {
    frame: frame::Video,
    logo: Option<Arc<LogoOverlay>>,
    opacity: f64,
    state: Timeline,
}

struct AudioFrame {
    frame: frame::Audio,
    hold_gain: bool,
    state: Timeline,
}

struct FrameSender<'a> {
    video: &'a PipelineQueue<VideoFrame>,
    audio: &'a PipelineQueue<AudioFrame>,
    state: Timeline,
    audio_size: usize,
    cfg: &'a OutputConfig,
    diagnostics: Option<&'a LiveInputDiagnostics>,
    defer_logo: bool,
    logo: Option<Arc<LogoOverlay>>,
    opacity: f64,
}

impl FrameOutput for FrameSender<'_> {
    fn set_frame_timeline(&mut self, timeline: &Timeline) {
        self.state = *timeline;
    }

    fn benchmarks_logo_overlay(&self) -> bool {
        !self.defer_logo
    }

    fn input_diagnostics(&self) -> Option<&LiveInputDiagnostics> {
        self.diagnostics
    }

    fn audio_frame_size(&self) -> usize {
        self.audio_size
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        let bytes = (0..frame.planes())
            .map(|index| frame.data(index).len())
            .sum();
        self.video.send(
            VideoFrame {
                frame: reference_video_frame(frame)?,
                logo: self.logo.clone(),
                opacity: self.opacity,
                state: self.state,
            },
            bytes,
            frame
                .pts()
                .map(|pts| pts.rescale(self.cfg.video_time_base, rescale::TIME_BASE)),
        )
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.encode_audio_with_gain_hold(frame, false)
    }

    fn encode_audio_with_gain_hold(&mut self, frame: &frame::Audio, hold_gain: bool) -> Result<()> {
        let bytes = frame.samples().saturating_mul(8);
        self.audio.send(
            AudioFrame {
                frame: reference_audio_frame(frame)?,
                hold_gain,
                state: self.state,
            },
            bytes,
            frame
                .pts()
                .map(|pts| pts.rescale(self.cfg.audio_time_base, rescale::TIME_BASE)),
        )
    }

    fn apply_logo_overlay(&mut self, frame: &mut frame::Video, logo: &LogoOverlay, opacity: f64) {
        if self.defer_logo {
            self.logo.get_or_insert_with(|| Arc::new(logo.clone()));
            self.opacity = opacity;
        } else {
            blend_logo(frame, logo, opacity);
        }
    }
}

struct CancelOnDrop<'a>(&'a PipelineControl);

impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[derive(Clone, Copy)]
struct WorkerContext<'a> {
    cfg: &'a OutputConfig,
    label: &'a str,
    initial: Timeline,
    plan: &'a Plan,
    control: &'a PipelineControl,
    video_packets: &'a PipelineQueue<Packet>,
    audio_packets: &'a PipelineQueue<Packet>,
    video_frames: &'a PipelineQueue<VideoFrame>,
    audio_frames: &'a PipelineQueue<AudioFrame>,
    progress: &'a Progress,
    benchmark: &'a Option<benchmark::BenchHandle>,
    diagnostics: Option<&'a LiveInputDiagnostics>,
    audio_size: usize,
    defer_logo: bool,
}
#[allow(clippy::too_many_arguments)]
pub(super) fn play<O: FrameOutput>(
    label: &str,
    input: format::context::Input,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    options: InputPlaybackOptions<'_>,
    external_audio: Option<&mut ExternalAudioInput>,
    plan: Plan,
) -> Result<()> {
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    let control = Arc::new(PipelineControl::default());
    let video_packets = PipelineQueue::<Packet>::new(PACKET_LIMITS, control.clone());
    let audio_packets = PipelineQueue::<Packet>::new(PACKET_LIMITS, control.clone());
    let video_frames = PipelineQueue::<VideoFrame>::new(VIDEO_LIMITS, control.clone());
    let audio_frames = PipelineQueue::<AudioFrame>::new(AUDIO_LIMITS, control.clone());
    let progress = Progress::new();
    let initial = *timeline;
    let audio_size = output.audio_frame_size();
    let defer_logo = !output.benchmarks_logo_overlay();
    let has_video = plan.video.is_some();
    let has_embedded_audio = plan.audio.is_some();
    let video_index = plan
        .video
        .as_ref()
        .map(|stream| (stream.index, stream.time_base));
    let audio_end_us = plan.audio.as_ref().and_then(|stream| stream.end_us);
    let audio_index = plan
        .audio
        .as_ref()
        .map(|stream| (stream.index, stream.time_base));
    let base_us = initial
        .video_pts
        .rescale(cfg.video_time_base, rescale::TIME_BASE)
        .min(
            initial
                .audio_pts
                .rescale(cfg.audio_time_base, rescale::TIME_BASE),
        );
    let output_clock = (!initial.source_timestamp_mode)
        .then(|| output.playout_clock())
        .flatten();
    let mut read_rate = ReadRate::new(
        output_clock.clone(),
        base_us,
        plan.source_origin_us.saturating_add(plan.seek_us),
    );

    if let Some(stream) = &plan.video {
        let rate = [stream.average_rate, stream.rate]
            .into_iter()
            .find(|rate| has_valid_frame_rate(*rate));

        if let Some(rate) = rate {
            read_rate.initial_burst_us = read_rate
                .initial_burst_us
                .max((1_000_000.0 / f64::from(rate)).ceil() as i64);
        }
    }

    let last_video_dts = plan
        .video
        .as_ref()
        .and_then(|spec| final_video_dts(&input, spec));
    let benchmark = benchmark::current();
    output.set_video_end(plan.video_end_pts)?;
    output.clear_vtt_subtitles()?;

    if let Some(media_path) = options.subtitles_media_path {
        benchmark::measure(Stage::Vtt, || {
            output.write_vtt_subtitles(
                media_path,
                initial.video_pts * 1_000 / i64::from(cfg.fps),
                plan.seek_us / 1_000,
            )
        })?;
    }

    if !has_video {
        output.video_decoded()?;
        output.video_finished()?;
        video_frames.close();
    }

    if !plan.has_audio {
        audio_frames.close();
    }

    if !has_video {
        video_packets.close();
    }

    if !has_embedded_audio {
        audio_packets.close();
    }

    log::trace!(channel = cfg.channel_id.unwrap_or_default(); r#"<span class="log-gray">[Input Pipeline]</span> generation=<span class="log-number">{generation}</span> started video=<span class="log-addr">{has_video}</span> audio=<span class="log-addr">{}</span>"#, plan.has_audio);
    let diagnostics_handle = output.input_diagnostics_handle();
    let result = thread::scope(|scope| -> Result<()> {
        let _cancel_guard = CancelOnDrop(&control);
        let diagnostics = diagnostics_handle.as_deref();
        let context = WorkerContext {
            cfg,
            label,
            initial,
            plan: &plan,
            control: &control,
            video_packets: &video_packets,
            audio_packets: &audio_packets,
            video_frames: &video_frames,
            audio_frames: &audio_frames,
            progress: &progress,
            benchmark: &benchmark,
            diagnostics,
            audio_size,
            defer_logo,
        };

        if let Some(spec) = &plan.video {
            scope.spawn(move || video::run(context, spec));
        }

        if plan.has_audio {
            scope.spawn(move || audio::run(context, external_audio, output_clock));
        }
        scope.spawn(move || {
            demux::run(
                context,
                input,
                read_rate,
                video_index,
                audio_index,
                audio_end_us,
                last_video_dts,
            );
        });
        // All FFmpeg input/decoder state belongs exclusively to the workers.
        // This coordinator never waits exclusively for one track: consume the
        // earlier available frame, allowing the other queue to keep draining.
        let result = coordinate(
            cfg,
            timeline,
            output,
            options.playback_control,
            &plan,
            &video_frames,
            &audio_frames,
            &control,
            has_embedded_audio,
            &progress,
        );
        control.cancel();
        result
    });
    log::trace!(channel = cfg.channel_id.unwrap_or_default(); r#"<span class="log-gray">[Input Pipeline]</span> generation=<span class="log-number">{generation}</span> stopped"#);

    result
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{
        PlaybackControl,
        playout::{PlaybackSkipped, play_clip},
    };

    #[derive(Default)]
    struct Capture {
        control: PlaybackControl,
        skip: bool,
        video_pts: Vec<i64>,
        first_audio: Option<Instant>,
        first_video: Option<Instant>,
        video_luma: Vec<u8>,
        audio_samples: Vec<f32>,
    }

    impl FrameOutput for Capture {
        fn audio_frame_size(&self) -> usize {
            1024
        }
        fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
            self.first_audio.get_or_insert_with(Instant::now);
            self.audio_samples.extend_from_slice(frame.plane::<f32>(0));

            Ok(())
        }
        fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
            self.first_video.get_or_insert_with(Instant::now);
            self.video_luma.push(frame.data(0)[0]);
            self.video_pts.push(frame.pts().unwrap());

            if self.skip {
                self.skip = false;
                self.control.skip_current();
            }

            Ok(())
        }
    }

    #[test]
    fn slow_video_decode_does_not_prevent_audio_processing() {
        let cfg = OutputConfig::new(64, 36, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut capture = Capture::default();
        let control = PlaybackControl::default();
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests_assets/storage/media_filler/filler_0.mp4"
        );
        let input = format::input(&path).unwrap();
        let mut video = StreamSpec::new(
            &input
                .streams()
                .best(ffmpeg_next::media::Type::Video)
                .unwrap(),
        );
        video.decode_delay_ms = 300;
        let audio = StreamSpec::new(
            &input
                .streams()
                .best(ffmpeg_next::media::Type::Audio)
                .unwrap(),
        );
        let logo_fade_plan = LogoFadePlan::none(0, &cfg);
        play(
            path,
            input,
            &cfg,
            &mut timeline,
            &mut capture,
            InputPlaybackOptions {
                seek_seconds: None,
                duration_seconds: Some(0.2),
                subtitles_media_path: None,
                logo_fade_plan,
                playback_control: &control,
                preserve_source_timestamps: false,
            },
            None,
            Plan {
                video: Some(video),
                audio: Some(audio),
                has_audio: true,
                seek_us: 0,
                source_origin_us: 0,
                video_limit_pts: Some(5),
                audio_limit_pts: Some(9600),
                video_end_pts: Some(5),
                audio_end_pts: Some(9600),
                audio_padding_end_pts: None,
                video_padding_end_pts: None,
                logo_fade_plan,
                media_fade_plan: MediaFadePlan::default(),
            },
        )
        .unwrap();
        let audio = capture.first_audio.unwrap();
        let video = capture.first_video.unwrap();
        assert!(video.duration_since(audio) >= Duration::from_millis(150));
        assert_eq!(capture.video_pts.len(), 5);
    }

    #[test]
    fn external_audio_chunks_preserve_the_same_samples_as_one_continuous_request() {
        let cfg = OutputConfig::new(64, 36, 25, 48_000);
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests_assets/storage/media_mix/audio.mp3"
        );
        let mut samples = Vec::new();

        for chunk in [48_000, 1024] {
            let mut timeline = Timeline::new();
            let mut output = Capture::default();
            let mut external =
                ExternalAudioInput::open(path, &cfg, &timeline, None, Some(1.0)).unwrap();
            let mut decoded = 0;

            while timeline.audio_pts < 48_000 {
                let end = (timeline.audio_pts + chunk).min(48_000);
                external
                    .decode_until(
                        &cfg,
                        &mut timeline,
                        &mut output,
                        Some(end),
                        &mut decoded,
                        &PlaybackControl::default(),
                    )
                    .unwrap();
            }
            assert_eq!(output.audio_samples.len(), 48_000);
            samples.push(output.audio_samples);
        }
        assert_eq!(samples[0], samples[1]);
    }

    #[test]
    fn unequal_tracks_without_a_complete_index_reach_eof() {
        ffmpeg_next::init().unwrap();

        for extension in ["mkv", "ts"] {
            for short_video in [true, false] {
                let path = std::env::temp_dir().join(format!(
                    "ff-engine-unequal-{}-{short_video}.{extension}",
                    std::process::id()
                ));
                let video = if short_video { "0.8" } else { "8" };
                let audio = if short_video { "8" } else { "0.8" };
                let result = std::process::Command::new("ffmpeg")
                    .args([
                        "-nostdin",
                        "-v",
                        "error",
                        "-y",
                        "-f",
                        "lavfi",
                        "-i",
                        &format!("testsrc2=s=64x36:r=25:d={video}"),
                        "-f",
                        "lavfi",
                        "-i",
                        &format!("sine=frequency=1000:sample_rate=48000:duration={audio}"),
                        "-c:v",
                        "libx264",
                        "-bf",
                        "2",
                        "-c:a",
                        "aac",
                    ])
                    .arg(&path)
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                let cfg = OutputConfig::new(64, 36, 25, 48_000);
                let mut capture = Capture::default();
                let mut timeline = Timeline::new();
                let started = Instant::now();
                play_clip(
                    path.to_str().unwrap(),
                    &cfg,
                    &mut timeline,
                    &mut capture,
                    None,
                    None,
                    None,
                    Default::default(),
                    &PlaybackControl::default(),
                )
                .unwrap();
                assert!(
                    started.elapsed() < Duration::from_secs(3),
                    "EOF must not require the five-second stall timeout"
                );
                assert!(
                    capture.video_pts.len() >= 200,
                    "{extension}, short_video={short_video}"
                );
                assert!(
                    capture.audio_samples.len() >= 384_000,
                    "{extension}, short_video={short_video}"
                );
                assert!((timeline.video_pts * 1920 - timeline.audio_pts).abs() < 1920);
                std::fs::remove_file(path).unwrap();
            }
        }
    }

    #[test]
    fn seek_with_b_frames_drains_short_video_before_long_audio() {
        ffmpeg_next::init().unwrap();
        let path = std::env::temp_dir().join(format!(
            "ff-engine-short-video-seek-{}.mp4",
            std::process::id()
        ));
        let result = std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x36:r=25:d=0.8",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:sample_rate=48000:duration=2",
                "-c:v",
                "libx264",
                "-bf",
                "2",
                "-c:a",
                "aac",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let cfg = OutputConfig::new(64, 36, 25, 48_000);
        let mut capture = Capture::default();
        play_clip(
            path.to_str().unwrap(),
            &cfg,
            &mut Timeline::new(),
            &mut capture,
            Some(0.2),
            Some(1.2),
            None,
            Default::default(),
            &PlaybackControl::default(),
        )
        .unwrap();
        assert_eq!(capture.video_pts.len(), 30);
        assert_eq!(capture.audio_samples.len(), 57_600);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn delayed_video_start_keeps_its_offset_to_audio() {
        ffmpeg_next::init().unwrap();
        let path = std::env::temp_dir().join(format!(
            "ff-engine-delayed-video-{}.mp4",
            std::process::id()
        ));
        let result = std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x36:r=25:d=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:sample_rate=48000:duration=1.2",
                "-vf",
                "setpts=PTS+0.2/TB",
                "-fps_mode",
                "passthrough",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-t",
                "1.2",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let cfg = OutputConfig::new(64, 36, 25, 48_000);
        let mut capture = Capture::default();
        play_clip(
            path.to_str().unwrap(),
            &cfg,
            &mut Timeline::new(),
            &mut capture,
            None,
            Some(1.2),
            None,
            Default::default(),
            &PlaybackControl::default(),
        )
        .unwrap();
        assert_eq!(capture.video_luma.len(), 30);
        assert!(capture.video_luma[..5].iter().all(|luma| *luma == 16));
        assert!(capture.video_luma[5..].iter().all(|luma| *luma > 60));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn skip_joins_pending_generation_before_the_next_clip() {
        let cfg = OutputConfig::new(64, 36, 25, 48_000);
        let mut timeline = Timeline::new();
        let control = PlaybackControl::default();
        let mut capture = Capture {
            control: control.clone(),
            skip: true,
            ..Default::default()
        };
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests_assets/storage/media_filler/filler_0.mp4"
        );
        let start = Instant::now();
        let error = play_clip(
            path,
            &cfg,
            &mut timeline,
            &mut capture,
            None,
            Some(3.0),
            None,
            Default::default(),
            &control,
        )
        .unwrap_err();
        assert!(error.is::<PlaybackSkipped>());
        assert!(start.elapsed() < Duration::from_secs(1));
        let before = capture.video_pts.len();
        play_clip(
            path,
            &cfg,
            &mut timeline,
            &mut capture,
            None,
            Some(0.2),
            None,
            Default::default(),
            &control,
        )
        .unwrap();
        assert_eq!(capture.video_pts.len() - before, 5);
        assert!(capture.video_pts.windows(2).all(|pts| pts[1] > pts[0]));
    }
}
