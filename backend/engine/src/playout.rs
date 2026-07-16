use std::{error::Error, fmt};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{
    Rational, Rescale, codec, format, frame, media,
    software::{resampling, scaling},
    util::{channel_layout::ChannelLayout, format::pixel::Pixel, format::sample::Sample},
};
use log::{debug, trace};

use crate::{
    LogoFade, PlaybackControl,
    benchmark::{self, Stage},
    compositor::{logo::*, text::TextOverlay},
    output::FrameOutput,
    utils::{
        config::{OutputConfig, TextOverlayState},
        helper::{even, open_media_input},
    },
};

const LOGO_FADE_SECONDS: f64 = 1.0;
const MIN_LOOP_REMAINING_SECONDS: f64 = 3.0;

#[derive(Debug)]
pub(crate) struct PlaybackSkipped;

impl fmt::Display for PlaybackSkipped {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("playback skipped")
    }
}

impl Error for PlaybackSkipped {}

#[derive(Debug)]
pub(crate) struct PlaybackRestart;

impl fmt::Display for PlaybackRestart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("playout restart requested")
    }
}

impl Error for PlaybackRestart {}

fn check_playback_control(playback_control: &PlaybackControl) -> Result<()> {
    if playback_control.take_restart() {
        return Err(PlaybackRestart.into());
    }
    if playback_control.take_skip_current() {
        return Err(PlaybackSkipped.into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) struct Timeline {
    video_pts: i64,
    audio_pts: i64,
    text_pts: i64,
    logo_opacity: f64,
}

impl Timeline {
    pub(crate) fn new() -> Self {
        Self {
            video_pts: 0,
            audio_pts: 0,
            text_pts: 0,
            logo_opacity: 1.0,
        }
    }

    pub(crate) fn video_pts(&self) -> i64 {
        self.video_pts
    }

    pub(crate) fn finish_logo_fade(&mut self, fade: LogoFade) {
        if fade.fade_out {
            self.logo_opacity = 0.0;
        } else if fade.fade_in {
            self.logo_opacity = 1.0;
        }
    }
}

/// Plays one file into the continuous output timeline.
///
/// Input PTS are replaced with continuous timeline PTS. If only one media type
/// exists, the missing counterpart is synthesized.
#[allow(clippy::too_many_arguments)]
pub(crate) fn play_clip<O: FrameOutput>(
    path: &str,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    seek_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    subtitles_media_path: Option<&str>,
    logo_fade: LogoFade,
    playback_control: &PlaybackControl,
) -> Result<()> {
    let logo_fade_plan = LogoFadePlan::new(timeline.video_pts, duration_seconds, cfg, logo_fade);

    let result = if let Some(duration_seconds) = duration_seconds.filter(|duration| *duration > 0.0)
    {
        play_looped_clip(
            path,
            cfg,
            timeline,
            output,
            seek_seconds,
            duration_seconds,
            subtitles_media_path,
            logo_fade_plan,
            playback_control,
        )
    } else {
        let ictx = open_media_input(path)?;
        play_opened_input(
            path,
            ictx,
            cfg,
            timeline,
            output,
            InputPlaybackOptions {
                seek_seconds,
                duration_seconds,
                subtitles_media_path,
                logo_fade_plan,
                playback_control,
            },
        )
    };

    if result.is_ok() {
        logo_fade_plan.finish(timeline);
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn play_looped_clip<O: FrameOutput>(
    path: &str,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    seek_seconds: Option<f64>,
    duration_seconds: f64,
    subtitles_media_path: Option<&str>,
    logo_fade_plan: LogoFadePlan,
    playback_control: &PlaybackControl,
) -> Result<()> {
    if !duration_seconds.is_finite() {
        return Err(anyhow!("clip duration must be a finite number"));
    }

    let mut remaining = duration_seconds;
    let mut first_iteration = true;
    let mut iterations = 0_u32;
    let minimum_progress = (1.0 / f64::from(cfg.fps)).min(1.0 / f64::from(cfg.sample_rate));

    while should_play_loop_iteration(first_iteration, remaining, minimum_progress) {
        check_playback_control(playback_control)?;
        let before_video_pts = timeline.video_pts;
        let before_audio_pts = timeline.audio_pts;
        let ictx = open_media_input(path)?;
        play_opened_input(
            path,
            ictx,
            cfg,
            timeline,
            output,
            InputPlaybackOptions {
                seek_seconds: first_iteration.then_some(seek_seconds).flatten(),
                duration_seconds: Some(remaining),
                subtitles_media_path: first_iteration.then_some(subtitles_media_path).flatten(),
                logo_fade_plan,
                playback_control,
            },
        )?;

        let elapsed = elapsed_timeline_seconds(cfg, timeline, before_video_pts, before_audio_pts);
        if elapsed <= minimum_progress {
            return Err(anyhow!(
                "{path} did not advance the playout timeline while looping to the requested duration"
            ));
        }

        remaining -= elapsed;
        first_iteration = false;
        iterations += 1;

        if remaining >= MIN_LOOP_REMAINING_SECONDS {
            debug!(
                "looping {path} to fill requested duration; iteration {iterations}, remaining {:.6} s",
                remaining
            );
        }
    }

    Ok(())
}

fn should_play_loop_iteration(
    first_iteration: bool,
    remaining: f64,
    minimum_progress: f64,
) -> bool {
    if first_iteration {
        remaining > minimum_progress
    } else {
        remaining >= MIN_LOOP_REMAINING_SECONDS
    }
}

fn elapsed_timeline_seconds(
    cfg: &OutputConfig,
    timeline: &Timeline,
    before_video_pts: i64,
    before_audio_pts: i64,
) -> f64 {
    let video_elapsed = (timeline.video_pts - before_video_pts).max(0) as f64 / f64::from(cfg.fps);
    let audio_elapsed =
        (timeline.audio_pts - before_audio_pts).max(0) as f64 / f64::from(cfg.sample_rate);

    video_elapsed.max(audio_elapsed)
}

pub(crate) struct InputPlaybackOptions<'a> {
    pub(crate) seek_seconds: Option<f64>,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) subtitles_media_path: Option<&'a str>,
    pub(crate) logo_fade_plan: LogoFadePlan,
    pub(crate) playback_control: &'a PlaybackControl,
}

#[derive(Clone, Copy)]
pub(crate) struct LogoFadePlan {
    fade_in: bool,
    fade_out: bool,
    start_pts: i64,
    end_pts: Option<i64>,
    frames: i64,
}

impl LogoFadePlan {
    pub(crate) fn none(start_pts: i64, cfg: &OutputConfig) -> Self {
        Self::new(start_pts, None, cfg, LogoFade::default())
    }

    fn new(
        start_pts: i64,
        duration_seconds: Option<f64>,
        cfg: &OutputConfig,
        fade: LogoFade,
    ) -> Self {
        let end_pts = duration_seconds
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .map(|duration| start_pts + (duration * f64::from(cfg.fps)).ceil() as i64);

        Self {
            fade_in: fade.fade_in,
            fade_out: fade.fade_out,
            start_pts,
            end_pts,
            frames: (LOGO_FADE_SECONDS * f64::from(cfg.fps)).round().max(1.0) as i64,
        }
    }

    fn with_end_pts(mut self, end_pts: Option<i64>) -> Self {
        if self.end_pts.is_none() {
            self.end_pts = end_pts;
        }
        self
    }

    fn opacity_at(self, pts: i64, current_opacity: f64) -> f64 {
        let mut opacity = current_opacity;

        if self.fade_in {
            let elapsed = (pts - self.start_pts).max(0);
            opacity = (elapsed as f64 / self.frames as f64).clamp(0.0, 1.0);
        }

        if self.fade_out
            && let Some(end_pts) = self.end_pts
        {
            let remaining = (end_pts - pts).max(0);
            opacity = opacity.min((remaining as f64 / self.frames as f64).clamp(0.0, 1.0));
        }

        opacity
    }

    fn finish(self, timeline: &mut Timeline) {
        timeline.finish_logo_fade(LogoFade {
            fade_in: self.fade_in,
            fade_out: self.fade_out,
        });
    }
}

pub(crate) fn play_opened_input<O: FrameOutput>(
    label: &str,
    mut ictx: format::context::Input,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    options: InputPlaybackOptions<'_>,
) -> Result<()> {
    let seek_seconds = options.seek_seconds;
    let seek_us = seek_seconds.map(seconds_to_microseconds).unwrap_or(0);
    if let Some(seek_seconds) = seek_seconds {
        seek_input(&mut ictx, seek_seconds)?;
    }

    let video_stream = ictx.streams().best(media::Type::Video);
    let audio_stream = ictx.streams().best(media::Type::Audio);
    if video_stream.is_none() && audio_stream.is_none() {
        return Err(anyhow!("{label} contains no audio or video stream"));
    }

    let duration_us = options.duration_seconds.map(seconds_to_microseconds);
    let video_limit_pts = duration_us.map(|duration_us| {
        timeline.video_pts
            + div_ceil(i128::from(duration_us) * i128::from(cfg.fps), 1_000_000) as i64
    });
    let audio_limit_pts = duration_us.map(|duration_us| {
        timeline.audio_pts
            + div_ceil(
                i128::from(duration_us) * i128::from(cfg.sample_rate),
                1_000_000,
            ) as i64
    });

    let video_duration_us = video_stream.as_ref().and_then(stream_duration_us);
    let video_end_pts = video_duration_us.map(|duration_us| {
        let duration_us = duration_us.saturating_sub(seek_us);
        timeline.video_pts
            + div_ceil(i128::from(duration_us) * i128::from(cfg.fps), 1_000_000) as i64
    });
    let logo_fade_plan = options
        .logo_fade_plan
        .with_end_pts(video_limit_pts.or(video_end_pts));

    let trim_start_us = (seek_us > 0).then_some(seek_us);
    let mut video = match video_stream {
        Some(ref stream) => Some(VideoDecoder::new(
            stream,
            cfg,
            label,
            trim_start_us,
            timeline.video_pts,
            timeline.text_pts,
            video_limit_pts.or(video_end_pts),
        )?),
        None => None,
    };
    let mut audio = match audio_stream {
        Some(ref stream) => Some(AudioDecoder::new(stream, cfg, trim_start_us)?),
        None => None,
    };

    let video_index = video_stream.as_ref().map(format::stream::Stream::index);
    let audio_index = audio_stream.as_ref().map(format::stream::Stream::index);
    let mut video_finished = video.is_none();
    let mut decoded_video_frames = 0_i64;
    let mut decoded_audio_samples = 0_i64;
    output.set_video_end(video_end_pts)?;
    if let Some(media_path) = options.subtitles_media_path {
        benchmark::measure(Stage::Vtt, || {
            output.write_vtt_subtitles(
                media_path,
                timeline.video_pts * 1_000 / i64::from(cfg.fps),
                seek_us / 1_000,
            )
        })?;
    }

    if video_finished {
        output.video_finished()?;
    }

    for (stream, packet) in ictx.packets() {
        check_playback_control(options.playback_control)?;
        if Some(stream.index()) == video_index {
            if !video_finished && let Some(video) = video.as_mut() {
                benchmark::measure(Stage::VideoDecode, || video.decoder.send_packet(&packet))?;
                receive_video_frames(
                    video,
                    timeline,
                    output,
                    &mut decoded_video_frames,
                    video_limit_pts,
                    logo_fade_plan,
                    options.playback_control,
                )?;
            }
        } else if Some(stream.index()) == audio_index
            && let Some(audio) = audio.as_mut()
        {
            benchmark::measure(Stage::AudioDecode, || audio.decoder.send_packet(&packet))?;
            receive_audio_frames(
                audio,
                timeline,
                output,
                &mut decoded_audio_samples,
                audio_limit_pts,
                options.playback_control,
            )?;
        }

        if duration_us.is_some()
            && video_limit_pts.is_none_or(|limit| timeline.video_pts >= limit)
            && audio_limit_pts.is_none_or(|limit| timeline.audio_pts >= limit)
        {
            break;
        }
    }

    finish_video(
        &mut video,
        timeline,
        output,
        &mut decoded_video_frames,
        &mut video_finished,
        video_limit_pts,
        logo_fade_plan,
        options.playback_control,
    )?;
    if let Some(audio) = audio.as_mut() {
        benchmark::measure(Stage::AudioDecode, || audio.decoder.send_eof())?;
        receive_audio_frames(
            audio,
            timeline,
            output,
            &mut decoded_audio_samples,
            audio_limit_pts,
            options.playback_control,
        )?;
        flush_audio_resampler(
            audio,
            timeline,
            output,
            &mut decoded_audio_samples,
            audio_limit_pts,
        )?;
    }

    if decoded_video_frames == 0 && decoded_audio_samples == 0 {
        return Err(anyhow!(
            "{label} produced no decodable audio or video frames"
        ));
    }

    let last_video_frame = video
        .as_ref()
        .and_then(|video| video.last_composited_frame.clone());

    synchronize_timeline(cfg, timeline, output, last_video_frame.as_ref())
}

fn seek_input(ictx: &mut format::context::Input, seek_seconds: f64) -> Result<()> {
    if !seek_seconds.is_finite() || seek_seconds < 0.0 {
        return Err(anyhow!("seek position must be a non-negative number"));
    }

    let seek_ts = seconds_to_microseconds(seek_seconds);
    ictx.seek(seek_ts, ..seek_ts)
        .or_else(|_| ictx.seek(seek_ts, ..))
        .with_context(|| format!("failed to seek first input to {seek_seconds:.3} seconds"))
}

fn seconds_to_microseconds(seconds: f64) -> i64 {
    (seconds * 1_000_000.0).round().max(0.0) as i64
}

#[allow(clippy::too_many_arguments)]
fn finish_video<O: FrameOutput>(
    video: &mut Option<VideoDecoder>,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    finished: &mut bool,
    limit_pts: Option<i64>,
    logo_fade_plan: LogoFadePlan,
    playback_control: &PlaybackControl,
) -> Result<()> {
    if *finished {
        return Ok(());
    }

    if let Some(video) = video.as_mut() {
        benchmark::measure(Stage::VideoDecode, || video.decoder.send_eof())?;
        receive_video_frames(
            video,
            timeline,
            output,
            decoded_frames,
            limit_pts,
            logo_fade_plan,
            playback_control,
        )?;
    }
    repeat_single_video_frame_to_limit(
        video,
        timeline,
        output,
        decoded_frames,
        limit_pts,
        logo_fade_plan,
        playback_control,
    )?;
    output.video_finished()?;
    *finished = true;
    Ok(())
}

fn repeat_single_video_frame_to_limit<O: FrameOutput>(
    video: &mut Option<VideoDecoder>,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    limit_pts: Option<i64>,
    logo_fade_plan: LogoFadePlan,
    playback_control: &PlaybackControl,
) -> Result<()> {
    let Some(limit_pts) = limit_pts else {
        return Ok(());
    };
    let Some(video) = video.as_mut() else {
        return Ok(());
    };
    let repeat_frames = single_frame_repeat_frames(*decoded_frames, timeline.video_pts, limit_pts);
    if repeat_frames == 0 {
        return Ok(());
    }
    let Some(frame) = video.last_output_frame.clone() else {
        return Ok(());
    };

    debug!(
        "holding single decoded video frame for {repeat_frames} frame(s) ({:.6} s)",
        repeat_frames as f64 / f64::from(video.output_fps)
    );

    while timeline.video_pts < limit_pts {
        check_playback_control(playback_control)?;
        let mut frame = frame.clone();
        apply_overlays(&mut frame, video, timeline, logo_fade_plan, output);
        frame.set_pts(Some(timeline.video_pts));
        output.encode_video(&frame)?;
        video.last_composited_frame = Some(frame);
        timeline.video_pts += 1;
        *decoded_frames += 1;
    }

    Ok(())
}

fn single_frame_repeat_frames(decoded_frames: i64, video_pts: i64, limit_pts: i64) -> i64 {
    if decoded_frames == 1 && video_pts < limit_pts {
        limit_pts - video_pts
    } else {
        0
    }
}

fn stream_duration_us(stream: &format::stream::Stream) -> Option<i64> {
    if stream.duration() > 0 {
        return Some(
            stream
                .duration()
                .rescale(stream.time_base(), Rational(1, 1_000_000)),
        );
    }

    let metadata = stream.metadata();
    metadata
        .get("DURATION")
        .or_else(|| metadata.get("duration"))
        .and_then(parse_duration_us)
}

fn parse_duration_us(duration: &str) -> Option<i64> {
    let mut parts = duration.split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    let duration_us = ((hours * 3_600.0 + minutes * 60.0 + seconds) * 1_000_000.0).round();
    (duration_us > 0.0).then_some(duration_us as i64)
}

fn receive_video_frames<O: FrameOutput>(
    video: &mut VideoDecoder,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    limit_pts: Option<i64>,
    logo_fade_plan: LogoFadePlan,
    playback_control: &PlaybackControl,
) -> Result<()> {
    let mut raw = frame::Video::empty();
    while benchmark::measure_success(Stage::VideoDecode, || video.decoder.receive_frame(&mut raw))
        .is_ok()
    {
        check_playback_control(playback_control)?;
        if limit_pts.is_some_and(|limit| timeline.video_pts >= limit) {
            return Ok(());
        }

        if is_before_trim_start(
            raw.timestamp().or_else(|| raw.pts()),
            video.frame_rate_converter.input_time_base,
            video.trim_start_us,
        ) {
            continue;
        }

        let output_frames = video
            .frame_rate_converter
            .output_frames(raw.timestamp().or_else(|| raw.pts()));
        if output_frames == 0 {
            continue;
        }

        let mut scaled = frame::Video::empty();
        benchmark::measure(Stage::Scale, || video.scaler.run(&raw, &mut scaled))?;
        let pristine = if video.needs_padding() {
            let mut padded = black_video_frame(video.output_width, video.output_height);
            benchmark::measure(Stage::Scale, || {
                copy_video_frame(
                    &scaled,
                    &mut padded,
                    video.x_offset,
                    video.y_offset,
                    video.scaled_width,
                    video.scaled_height,
                );
            });
            padded
        } else {
            scaled
        };
        // Only the first decoded frame can become the single-frame repeat
        // source (see `repeat_single_video_frame_to_limit`); keeping a copy of
        // every frame would cost a full-frame memcpy per output frame.
        if *decoded_frames == 0 {
            video.last_output_frame = Some(pristine.clone());
        }
        for _ in 0..output_frames {
            check_playback_control(playback_control)?;
            if limit_pts.is_some_and(|limit| timeline.video_pts >= limit) {
                return Ok(());
            }

            // Overlays must be blended onto a fresh copy: blending in place
            // onto the shared buffer would stack the logo (and scrolling text
            // positions) on top of each other for duplicated frames during
            // frame-rate up-conversion.
            let mut frame = pristine.clone();
            apply_overlays(&mut frame, video, timeline, logo_fade_plan, output);
            frame.set_pts(Some(timeline.video_pts));
            output.encode_video(&frame)?;
            video.last_composited_frame = Some(frame);
            timeline.video_pts += 1;
            *decoded_frames += 1;
        }
    }
    Ok(())
}

fn apply_overlays(
    frame: &mut frame::Video,
    video: &mut VideoDecoder,
    timeline: &mut Timeline,
    logo_fade_plan: LogoFadePlan,
    output: &mut impl FrameOutput,
) {
    let opacity = logo_fade_plan.opacity_at(timeline.video_pts, timeline.logo_opacity);
    timeline.logo_opacity = opacity;

    if let Some(logo) = &video.logo {
        if output.benchmarks_logo_overlay() {
            benchmark::measure_overlay(Stage::LogoOverlay, logo.width, logo.height, || {
                output.apply_logo_overlay(frame, logo, opacity);
            });
        } else {
            output.apply_logo_overlay(frame, logo, opacity);
        }
    }
    if let Some(text) = &mut video.text {
        let (width, height) = text.dimensions();
        benchmark::measure_overlay(Stage::TextStatic, width, height, || {
            text.blend(frame, timeline.video_pts, timeline.text_pts);
        });
    }
    video.update_runtime_text(timeline.video_pts, timeline.text_pts);
    if let Some(text) = &mut video.runtime_text {
        let (width, height) = text.dimensions();
        benchmark::measure_overlay(Stage::TextRuntime, width, height, || {
            text.blend(frame, timeline.video_pts, timeline.text_pts);
        });
    }
    timeline.text_pts += 1;
}

fn receive_audio_frames<O: FrameOutput>(
    audio: &mut AudioDecoder,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_samples: &mut i64,
    limit_pts: Option<i64>,
    playback_control: &PlaybackControl,
) -> Result<()> {
    let mut raw = frame::Audio::empty();
    while benchmark::measure_success(Stage::AudioDecode, || audio.decoder.receive_frame(&mut raw))
        .is_ok()
    {
        check_playback_control(playback_control)?;
        if limit_pts.is_some_and(|limit| timeline.audio_pts >= limit) {
            return Ok(());
        }

        if is_before_trim_start(
            raw.timestamp().or_else(|| raw.pts()),
            audio.input_time_base,
            audio.trim_start_us,
        ) {
            continue;
        }

        if raw.channel_layout().is_empty() {
            raw.set_channel_layout(audio.input_channel_layout);
        }

        let mut converted = frame::Audio::empty();
        benchmark::measure(Stage::AudioProcess, || {
            audio.resampler.run(&raw, &mut converted)
        })?;
        let samples = converted.samples() as i64;
        converted.set_pts(Some(timeline.audio_pts));
        output.encode_audio(&converted)?;
        timeline.audio_pts += samples;
        *decoded_samples += samples;
    }
    Ok(())
}

/// Drains samples still buffered inside the resampler after decoder EOF;
/// without this, sample-rate conversion silently truncates a few milliseconds
/// of audio at the end of every clip.
fn flush_audio_resampler<O: FrameOutput>(
    audio: &mut AudioDecoder,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_samples: &mut i64,
    limit_pts: Option<i64>,
) -> Result<()> {
    loop {
        if limit_pts.is_some_and(|limit| timeline.audio_pts >= limit) {
            return Ok(());
        }

        let mut converted = frame::Audio::new(
            Sample::F32(format::sample::Type::Planar),
            output.audio_frame_size().max(1),
            ChannelLayout::STEREO,
        );
        let delay = benchmark::measure(Stage::AudioProcess, || {
            audio.resampler.flush(&mut converted)
        })?;
        let samples = converted.samples() as i64;
        if samples == 0 {
            return Ok(());
        }

        converted.set_pts(Some(timeline.audio_pts));
        output.encode_audio(&converted)?;
        timeline.audio_pts += samples;
        *decoded_samples += samples;

        if delay.is_none() {
            return Ok(());
        }
    }
}

fn is_before_trim_start(
    timestamp: Option<i64>,
    time_base: Rational,
    trim_start_us: Option<i64>,
) -> bool {
    let Some(trim_start_us) = trim_start_us else {
        return false;
    };
    let Some(timestamp) = timestamp else {
        return false;
    };

    timestamp.rescale(time_base, Rational(1, 1_000_000)) < trim_start_us
}

struct VideoDecoder {
    decoder: codec::decoder::Video,
    scaler: scaling::Context,
    output_width: u32,
    output_height: u32,
    scaled_width: u32,
    scaled_height: u32,
    x_offset: u32,
    y_offset: u32,
    logo: Option<LogoOverlay>,
    text: Option<TextOverlay>,
    runtime_text_state: TextOverlayState,
    runtime_text_revision: u64,
    runtime_text: Option<TextOverlay>,
    label: String,
    frame_rate_converter: FrameRateConverter,
    output_fps: u32,
    trim_start_us: Option<i64>,
    last_output_frame: Option<frame::Video>,
    last_composited_frame: Option<frame::Video>,
}

impl VideoDecoder {
    fn new(
        stream: &format::stream::Stream,
        cfg: &OutputConfig,
        label: &str,
        trim_start_us: Option<i64>,
        start_pts: i64,
        scroll_pts: i64,
        end_pts: Option<i64>,
    ) -> Result<Self> {
        let mut ctx = codec::context::Context::from_parameters(stream.parameters())?;
        ctx.set_threading(codec::threading::Config::kind(
            codec::threading::Type::Frame,
        ));
        let decoder = ctx.decoder().video()?;
        let scale = VideoScale::new(decoder.width(), decoder.height(), cfg);
        let scaler = scaling::Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::YUV420P,
            scale.scaled_width,
            scale.scaled_height,
            scaling::flag::Flags::BILINEAR,
        )?;
        let runtime_text_snapshot = cfg.text_overlay_state.snapshot_at(scroll_pts);
        let runtime_text = runtime_text_snapshot
            .config
            .as_ref()
            .map(|text| {
                let text_start_pts = runtime_text_snapshot.start_pts.unwrap_or(scroll_pts);
                TextOverlay::load(
                    text,
                    label,
                    cfg.width,
                    cfg.height,
                    cfg.fps,
                    start_pts,
                    text_start_pts,
                    None,
                )
            })
            .transpose()?
            .flatten();

        Ok(Self {
            decoder,
            scaler,
            output_width: scale.output_width,
            output_height: scale.output_height,
            scaled_width: scale.scaled_width,
            scaled_height: scale.scaled_height,
            x_offset: scale.x_offset,
            y_offset: scale.y_offset,
            logo: cfg
                .logo
                .as_ref()
                .map(|logo| LogoOverlay::load(logo, cfg.width, cfg.height))
                .transpose()?,
            text: cfg
                .text
                .as_ref()
                .map(|text| {
                    TextOverlay::load(
                        text, label, cfg.width, cfg.height, cfg.fps, start_pts, 0, end_pts,
                    )
                })
                .transpose()?
                .flatten(),
            runtime_text_state: cfg.text_overlay_state.clone(),
            runtime_text_revision: runtime_text_snapshot.revision,
            runtime_text,
            label: label.to_string(),
            frame_rate_converter: FrameRateConverter::new(stream.time_base(), cfg.fps),
            output_fps: cfg.fps,
            trim_start_us,
            last_output_frame: None,
            last_composited_frame: None,
        })
    }

    fn needs_padding(&self) -> bool {
        self.scaled_width != self.output_width
            || self.scaled_height != self.output_height
            || self.x_offset != 0
            || self.y_offset != 0
    }

    fn update_runtime_text(&mut self, pts: i64, scroll_pts: i64) {
        let snapshot = self.runtime_text_state.snapshot_at(scroll_pts);
        if snapshot.revision == self.runtime_text_revision {
            return;
        }
        self.runtime_text_revision = snapshot.revision;
        self.runtime_text = snapshot.config.and_then(|config| {
            let text_start_pts = snapshot.start_pts.unwrap_or(scroll_pts);
            TextOverlay::load(
                &config,
                &self.label,
                self.output_width,
                self.output_height,
                self.output_fps,
                pts,
                text_start_pts,
                None,
            )
            .map_err(|error| {
                debug!("failed to render runtime text overlay: {error:#}");
                error
            })
            .ok()
            .flatten()
        });
    }
}

#[derive(Debug, Clone, Copy)]
struct VideoScale {
    output_width: u32,
    output_height: u32,
    scaled_width: u32,
    scaled_height: u32,
    x_offset: u32,
    y_offset: u32,
}

impl VideoScale {
    fn new(input_width: u32, input_height: u32, cfg: &OutputConfig) -> Self {
        let (scaled_width, scaled_height) =
            fit_dimensions(cfg.width, cfg.height, input_width, input_height);
        let x_offset = even((cfg.width.saturating_sub(scaled_width)) / 2);
        let y_offset = even((cfg.height.saturating_sub(scaled_height)) / 2);

        Self {
            output_width: cfg.width,
            output_height: cfg.height,
            scaled_width,
            scaled_height,
            x_offset,
            y_offset,
        }
    }
}

fn fit_dimensions(
    max_width: u32,
    max_height: u32,
    aspect_width: u32,
    aspect_height: u32,
) -> (u32, u32) {
    let max_width = max_width.max(2);
    let max_height = max_height.max(2);
    let width_limited_height =
        ((u64::from(max_width) * u64::from(aspect_height)) / u64::from(aspect_width)) as u32;
    if width_limited_height <= max_height {
        (even(max_width).max(2), even(width_limited_height).max(2))
    } else {
        let height_limited_width =
            ((u64::from(max_height) * u64::from(aspect_width)) / u64::from(aspect_height)) as u32;
        (even(height_limited_width).max(2), even(max_height).max(2))
    }
}

struct FrameRateConverter {
    input_time_base: Rational,
    output_time_base: Rational,
    first_timestamp: Option<i64>,
    next_output_frame: i64,
}

impl FrameRateConverter {
    fn new(input_time_base: Rational, output_fps: u32) -> Self {
        Self {
            input_time_base,
            output_time_base: Rational(1, output_fps as i32),
            first_timestamp: None,
            next_output_frame: 0,
        }
    }

    fn output_frames(&mut self, timestamp: Option<i64>) -> i64 {
        let Some(timestamp) = timestamp else {
            self.next_output_frame += 1;
            return 1;
        };

        let first_timestamp = *self.first_timestamp.get_or_insert(timestamp);
        let relative_timestamp = timestamp.saturating_sub(first_timestamp).max(0);
        let target_frame = relative_timestamp
            .rescale(self.input_time_base, self.output_time_base)
            .max(0);
        let output_frames = (target_frame + 1 - self.next_output_frame).max(0);
        self.next_output_frame += output_frames;
        output_frames
    }
}

struct AudioDecoder {
    decoder: codec::decoder::Audio,
    resampler: resampling::Context,
    input_channel_layout: ChannelLayout,
    input_time_base: Rational,
    trim_start_us: Option<i64>,
}

impl AudioDecoder {
    fn new(
        stream: &format::stream::Stream,
        cfg: &OutputConfig,
        trim_start_us: Option<i64>,
    ) -> Result<Self> {
        let ctx = codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = ctx.decoder().audio()?;
        let channel_layout = audio_channel_layout(&decoder);
        let resampler = resampling::Context::get(
            decoder.format(),
            channel_layout,
            decoder.rate(),
            Sample::F32(format::sample::Type::Planar),
            ChannelLayout::STEREO,
            cfg.sample_rate,
        )?;
        Ok(Self {
            decoder,
            resampler,
            input_channel_layout: channel_layout,
            input_time_base: stream.time_base(),
            trim_start_us,
        })
    }
}

fn audio_channel_layout(decoder: &codec::decoder::Audio) -> ChannelLayout {
    let channel_layout = decoder.channel_layout();
    if channel_layout.is_empty() {
        ChannelLayout::default(i32::from(decoder.channels()).max(1))
    } else {
        channel_layout
    }
}

pub(crate) fn write_fallback<O: FrameOutput>(
    label: &str,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    duration: f64,
    playback_control: &PlaybackControl,
) -> Result<()> {
    let video_end = timeline.video_pts + (duration * f64::from(cfg.fps)).ceil() as i64;
    let audio_end = timeline.audio_pts + (duration * f64::from(cfg.sample_rate)).ceil() as i64;
    let mut overlays = FallbackOverlays::new(
        label,
        cfg,
        timeline.video_pts,
        timeline.text_pts,
        Some(video_end),
    )?;

    while timeline.video_pts < video_end || timeline.audio_pts < audio_end {
        check_playback_control(playback_control)?;
        let video_time = timeline.video_pts as f64 / f64::from(cfg.fps);
        let audio_time = timeline.audio_pts as f64 / f64::from(cfg.sample_rate);

        if timeline.video_pts < video_end
            && (timeline.audio_pts >= audio_end || video_time <= audio_time)
        {
            write_black_frames(cfg, timeline, output, 1, Some(&mut overlays))?;
        } else {
            let remaining = (audio_end - timeline.audio_pts) as usize;
            let samples = remaining.min(output.audio_frame_size().max(1));
            write_silence_frame(cfg, timeline, output, samples)?;
        }
    }

    synchronize_timeline(cfg, timeline, output, None)
}

fn synchronize_timeline<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    last_video_frame: Option<&frame::Video>,
) -> Result<()> {
    let (video_frames, audio_samples) = padding_to_sync(
        timeline.video_pts,
        timeline.audio_pts,
        cfg.fps,
        cfg.sample_rate,
    )?;

    if video_frames > 0 {
        trace!(
            "padding video with {video_frames} frame(s) ({:.6} s) to synchronize the timeline",
            video_frames as f64 / f64::from(cfg.fps)
        );
    }
    write_padding_video_frames(cfg, timeline, output, video_frames, last_video_frame)?;

    if audio_samples > 0 {
        trace!(
            "padding audio with {audio_samples} silent sample(s) ({:.6} s) to synchronize the timeline",
            audio_samples as f64 / f64::from(cfg.sample_rate)
        );
    }
    write_silence(cfg, timeline, output, audio_samples)
}

fn padding_to_sync(
    video_pts: i64,
    audio_pts: i64,
    fps: u32,
    sample_rate: u32,
) -> Result<(i64, i64)> {
    if fps == 0 || sample_rate == 0 {
        return Err(anyhow!("fps and sample rate must be greater than zero"));
    }

    let fps = i128::from(fps);
    let sample_rate = i128::from(sample_rate);
    let mut video_end = i128::from(video_pts);
    let audio_end = i128::from(audio_pts);
    let mut video_padding = 0_i128;
    let mut audio_padding = 0_i128;

    if video_end * sample_rate < audio_end * fps {
        let target = div_ceil(audio_end * fps, sample_rate);
        video_padding = target - video_end;
        video_end = target;
    }

    if audio_end * fps < video_end * sample_rate {
        let target = div_ceil(video_end * sample_rate, fps);
        audio_padding = target - audio_end;
    }

    let video_padding =
        i64::try_from(video_padding).map_err(|_| anyhow!("video padding exceeds i64"))?;
    let audio_padding =
        i64::try_from(audio_padding).map_err(|_| anyhow!("audio padding exceeds i64"))?;

    Ok((video_padding, audio_padding))
}

fn div_ceil(numerator: i128, denominator: i128) -> i128 {
    (numerator + denominator - 1) / denominator
}

fn write_black_frames<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    frames: i64,
    mut overlays: Option<&mut FallbackOverlays>,
) -> Result<()> {
    for _ in 0..frames {
        let mut black = black_video_frame_for_config(cfg);
        if let Some(overlays) = overlays.as_mut() {
            overlays.apply(&mut black, timeline);
        }
        black.set_pts(Some(timeline.video_pts));
        output.encode_video(&black)?;
        timeline.video_pts += 1;
    }
    Ok(())
}

struct FallbackOverlays {
    text: Option<TextOverlay>,
    runtime_text_state: TextOverlayState,
    runtime_text_revision: u64,
    runtime_text: Option<TextOverlay>,
    label: String,
    output_width: u32,
    output_height: u32,
    output_fps: u32,
}

impl FallbackOverlays {
    fn new(
        label: &str,
        cfg: &OutputConfig,
        fade_start_pts: i64,
        scroll_pts: i64,
        end_pts: Option<i64>,
    ) -> Result<Self> {
        let runtime_text_snapshot = cfg.text_overlay_state.snapshot_at(scroll_pts);
        let runtime_text = runtime_text_snapshot
            .config
            .as_ref()
            .map(|text| {
                let text_start_pts = runtime_text_snapshot.start_pts.unwrap_or(scroll_pts);
                TextOverlay::load(
                    text,
                    label,
                    cfg.width,
                    cfg.height,
                    cfg.fps,
                    fade_start_pts,
                    text_start_pts,
                    None,
                )
            })
            .transpose()?
            .flatten();

        Ok(Self {
            text: cfg
                .text
                .as_ref()
                .map(|text| {
                    TextOverlay::load(
                        text,
                        label,
                        cfg.width,
                        cfg.height,
                        cfg.fps,
                        fade_start_pts,
                        0,
                        end_pts,
                    )
                })
                .transpose()?
                .flatten(),
            runtime_text_state: cfg.text_overlay_state.clone(),
            runtime_text_revision: runtime_text_snapshot.revision,
            runtime_text,
            label: label.to_string(),
            output_width: cfg.width,
            output_height: cfg.height,
            output_fps: cfg.fps,
        })
    }

    fn apply(&mut self, frame: &mut frame::Video, timeline: &mut Timeline) {
        if let Some(text) = &mut self.text {
            text.blend(frame, timeline.video_pts, timeline.text_pts);
        }
        self.update_runtime_text(timeline.video_pts, timeline.text_pts);
        if let Some(text) = &mut self.runtime_text {
            text.blend(frame, timeline.video_pts, timeline.text_pts);
        }
        timeline.text_pts += 1;
    }

    fn update_runtime_text(&mut self, pts: i64, scroll_pts: i64) {
        let snapshot = self.runtime_text_state.snapshot_at(scroll_pts);
        if snapshot.revision == self.runtime_text_revision {
            return;
        }
        self.runtime_text_revision = snapshot.revision;
        self.runtime_text = snapshot.config.and_then(|config| {
            let text_start_pts = snapshot.start_pts.unwrap_or(scroll_pts);
            TextOverlay::load(
                &config,
                &self.label,
                self.output_width,
                self.output_height,
                self.output_fps,
                pts,
                text_start_pts,
                None,
            )
            .map_err(|error| {
                debug!("failed to render fallback runtime text overlay: {error:#}");
                error
            })
            .ok()
            .flatten()
        });
    }
}

fn write_padding_video_frames<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    frames: i64,
    last_video_frame: Option<&frame::Video>,
) -> Result<()> {
    if let Some(last_video_frame) = last_video_frame {
        for _ in 0..frames {
            let mut frame = last_video_frame.clone();
            frame.set_pts(Some(timeline.video_pts));
            output.encode_video(&frame)?;
            timeline.video_pts += 1;
        }
        Ok(())
    } else {
        write_black_frames(cfg, timeline, output, frames, None)
    }
}

fn black_video_frame_for_config(cfg: &OutputConfig) -> frame::Video {
    black_video_frame(cfg.width, cfg.height)
}

fn black_video_frame(width: u32, height: u32) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P, width, height);
    fill_plane(&mut frame, 0, 16);
    fill_plane(&mut frame, 1, 128);
    fill_plane(&mut frame, 2, 128);
    frame
}

fn fill_plane(frame: &mut frame::Video, plane: usize, value: u8) {
    let height = if plane == 0 {
        frame.height()
    } else {
        frame.height() / 2
    } as usize;
    let width = if plane == 0 {
        frame.width()
    } else {
        frame.width() / 2
    } as usize;
    let stride = frame.stride(plane);
    let data = frame.data_mut(plane);
    for y in 0..height {
        let start = y * stride;
        data[start..start + width].fill(value);
    }
}

fn copy_video_frame(
    source: &frame::Video,
    target: &mut frame::Video,
    x_offset: u32,
    y_offset: u32,
    width: u32,
    height: u32,
) {
    for plane in 0..target.planes() {
        let chroma = if plane == 0 { 1 } else { 2 };
        let plane_x = (x_offset / chroma) as usize;
        let plane_y = (y_offset / chroma) as usize;
        let plane_width = (width / chroma) as usize;
        let plane_height = (height / chroma) as usize;
        let source_stride = source.stride(plane);
        let target_stride = target.stride(plane);
        let source_data = source.data(plane);
        let target_data = target.data_mut(plane);

        for y in 0..plane_height {
            let source_start = y * source_stride;
            let target_start = (plane_y + y) * target_stride + plane_x;
            target_data[target_start..target_start + plane_width]
                .copy_from_slice(&source_data[source_start..source_start + plane_width]);
        }
    }
}

fn write_silence_frame<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    samples: usize,
) -> Result<()> {
    let mut frame = frame::Audio::new(
        Sample::F32(format::sample::Type::Planar),
        samples,
        ChannelLayout::STEREO,
    );
    frame.set_rate(cfg.sample_rate);
    frame.set_pts(Some(timeline.audio_pts));
    for plane in 0..frame.planes() {
        frame.plane_mut::<f32>(plane).fill(0.0);
    }
    output.encode_audio(&frame)?;
    timeline.audio_pts += samples as i64;
    Ok(())
}

fn write_silence<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    mut samples: i64,
) -> Result<()> {
    let frame_samples = output.audio_frame_size().max(1);
    while samples > 0 {
        let current_samples = samples.min(frame_samples as i64) as usize;
        write_silence_frame(cfg, timeline, output, current_samples)?;
        samples -= current_samples as i64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use ffmpeg_next::frame;

    use super::{
        FrameRateConverter, LogoFade, PlaybackControl, Rational, Timeline, fit_dimensions,
        padding_to_sync, parse_duration_us, play_clip, should_play_loop_iteration,
        single_frame_repeat_frames,
    };
    use crate::{output::FrameOutput, utils::config::OutputConfig};

    #[derive(Default)]
    struct RecordingOutput {
        video_frames: Vec<(u32, u32, i64)>,
    }

    impl FrameOutput for RecordingOutput {
        fn audio_frame_size(&self) -> usize {
            1024
        }

        fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
            self.video_frames.push((
                frame.width(),
                frame.height(),
                frame.pts().unwrap_or_default(),
            ));
            Ok(())
        }

        fn encode_audio(&mut self, _frame: &frame::Audio) -> Result<()> {
            Ok(())
        }
    }

    fn media_mix_asset(name: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/assets/storage/media_mix")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn pads_short_audio_to_video_duration() {
        assert_eq!(padding_to_sync(50, 95_000, 25, 48_000).unwrap(), (0, 1_000));
    }

    #[test]
    fn pads_short_video_to_audio_duration() {
        assert_eq!(padding_to_sync(49, 96_000, 25, 48_000).unwrap(), (1, 0));
    }

    #[test]
    fn repeats_single_decoded_video_frame_until_requested_duration() {
        assert_eq!(single_frame_repeat_frames(1, 1, 250), 249);
        assert_eq!(single_frame_repeat_frames(2, 2, 250), 0);
        assert_eq!(single_frame_repeat_frames(1, 250, 250), 0);
    }

    #[test]
    fn looped_clip_allows_small_remaining_duration() {
        assert!(should_play_loop_iteration(true, 0.5, 0.04));
        assert!(!should_play_loop_iteration(false, 2.999, 0.04));
        assert!(should_play_loop_iteration(false, 3.0, 0.04));
    }

    #[test]
    fn rounds_both_streams_to_a_shared_boundary() {
        assert_eq!(padding_to_sync(30, 44_101, 30, 44_100).unwrap(), (1, 1_469));
    }

    #[test]
    fn fits_four_by_three_into_sixteen_by_nine() {
        assert_eq!(fit_dimensions(1024, 576, 640, 480), (768, 576));
    }

    #[test]
    fn fits_vertical_video_into_sixteen_by_nine() {
        assert_eq!(fit_dimensions(1024, 576, 1080, 1920), (324, 576));
    }

    #[test]
    fn plays_two_different_video_sizes_with_stable_desktop_output_size() {
        let cfg = OutputConfig::new(1280, 720, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();
        let playback_control = PlaybackControl::default();
        let first = media_mix_asset("aspect_4-3_30FPS.mp4");
        let second = media_mix_asset("aspect_9-16_50FPS.mp4");

        play_clip(
            &first,
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(0.2),
            None,
            LogoFade::default(),
            &playback_control,
        )
        .unwrap();
        let first_frame_count = output.video_frames.len();
        assert!(first_frame_count > 0);

        play_clip(
            &second,
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(0.2),
            None,
            LogoFade::default(),
            &playback_control,
        )
        .unwrap();
        assert!(output.video_frames.len() > first_frame_count);

        assert!(
            output
                .video_frames
                .iter()
                .all(|(width, height, _)| (*width, *height) == (1280, 720))
        );
        assert!(
            output
                .video_frames
                .windows(2)
                .all(|frames| frames[1].2 == frames[0].2 + 1)
        );
        assert_eq!(fit_dimensions(1280, 720, 768, 576), (960, 720));
        assert_eq!(fit_dimensions(1280, 720, 720, 1280), (404, 720));
    }

    #[test]
    fn rejects_invalid_output_rates() {
        assert!(padding_to_sync(1, 1, 0, 48_000).is_err());
        assert!(padding_to_sync(1, 1, 25, 0).is_err());
    }

    #[test]
    fn converts_24_fps_to_25_fps() {
        let mut converter = FrameRateConverter::new(Rational(1, 24), 25);
        let output_frames = (0..240)
            .map(|timestamp| converter.output_frames(Some(timestamp)))
            .sum::<i64>();

        assert_eq!(output_frames, 250);
    }

    #[test]
    fn converts_30_fps_to_25_fps() {
        let mut converter = FrameRateConverter::new(Rational(1, 30), 25);
        let output_counts = (0..300)
            .map(|timestamp| converter.output_frames(Some(timestamp)))
            .collect::<Vec<_>>();

        assert_eq!(output_counts.iter().sum::<i64>(), 250);
        assert_eq!(
            output_counts.iter().filter(|frames| **frames == 0).count(),
            50
        );
    }

    #[test]
    fn parses_stream_duration_metadata() {
        assert_eq!(parse_duration_us("00:00:12.000000000"), Some(12_000_000));
        assert_eq!(parse_duration_us("01:02:03.500"), Some(3_723_500_000));
        assert_eq!(parse_duration_us("invalid"), None);
    }
}
