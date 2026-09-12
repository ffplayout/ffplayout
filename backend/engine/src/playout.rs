use std::{error::Error, fmt};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{
    Rational, Rescale, codec, format, frame, media,
    software::{resampling, scaling},
    util::{channel_layout::ChannelLayout, format::pixel::Pixel, format::sample::Sample},
};
use log::{debug, trace, warn};

use crate::{
    LogoFade, PlaybackControl,
    benchmark::{self, Stage},
    compositor::{logo::*, text::TextOverlay},
    output::FrameOutput,
    utils::{
        config::{OutputConfig, TextOverlayState},
        ffmpeg::{make_video_frame_writable, reference_video_frame},
        helper::{even, is_live_input, open_media_input},
    },
};

const LOGO_FADE_SECONDS: f64 = 1.0;
const MEDIA_FADE_SECONDS: f64 = 1.0;
const MEDIA_DURATION_TOLERANCE_US: i64 = 500_000;
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

pub(crate) fn check_playback_control(playback_control: &PlaybackControl) -> Result<()> {
    if playback_control.is_shutdown() || playback_control.take_restart() {
        return Err(PlaybackRestart.into());
    }
    if playback_control.take_skip_current() || playback_control.take_navigation() {
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
    // Live ingest preserves source PTS until the receiver maps both tracks
    // onto the continuous playout timeline. File playback always uses the
    // normal monotonically increasing timeline values.
    source_timestamp_mode: bool,
}

impl Timeline {
    pub(crate) fn new() -> Self {
        Self {
            video_pts: 0,
            audio_pts: 0,
            text_pts: 0,
            logo_opacity: 1.0,
            source_timestamp_mode: false,
        }
    }

    pub(crate) fn video_pts(&self) -> i64 {
        self.video_pts
    }

    /// Move the persistent playout timeline to an externally produced media
    /// position. Live ingest advances outside the suspended playlist decoder;
    /// the next playlist clip must start from that actual output position.
    pub(crate) fn reanchor(&mut self, video_pts: i64, audio_pts: i64) {
        self.video_pts = video_pts;
        self.audio_pts = audio_pts;
        self.text_pts = video_pts;
        self.source_timestamp_mode = false;
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
#[cfg(test)]
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
    play_clip_with_external_audio(
        path,
        None,
        cfg,
        timeline,
        output,
        seek_seconds,
        duration_seconds,
        subtitles_media_path,
        logo_fade,
        playback_control,
    )
}

/// Plays a video source with an optional external audio source. The audio input
/// is decoded on demand while video advances, so encoded outputs receive
/// timestamp-interleaved audio/video packets.
#[allow(clippy::too_many_arguments)]
pub(crate) fn play_clip_with_external_audio<O: FrameOutput>(
    path: &str,
    external_audio_path: Option<&str>,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    seek_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    subtitles_media_path: Option<&str>,
    logo_fade: LogoFade,
    playback_control: &PlaybackControl,
) -> Result<()> {
    let source_is_live = is_live_input(path);
    // Playlist timing can carry a source offset. Live transports have no
    // stable timeline to seek in, so always begin at their current edge.
    let source_seek_seconds = (!source_is_live).then_some(seek_seconds).flatten();
    let logo_fade_plan = LogoFadePlan::new(timeline.video_pts, duration_seconds, cfg, logo_fade);
    let mut external_audio = external_audio_path
        .map(|audio_path| {
            ExternalAudioInput::open(audio_path, cfg, timeline, seek_seconds, duration_seconds)
                .with_context(|| format!("failed to open external audio {audio_path} for {path}"))
        })
        .transpose()?;

    let result = if should_loop_input(source_is_live, duration_seconds) {
        let duration_seconds = duration_seconds.expect("checked by should_loop_input");
        play_looped_clip(
            path,
            cfg,
            timeline,
            output,
            source_seek_seconds,
            duration_seconds,
            subtitles_media_path,
            logo_fade_plan,
            playback_control,
            external_audio.as_mut(),
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
                seek_seconds: source_seek_seconds,
                duration_seconds,
                subtitles_media_path,
                logo_fade_plan,
                playback_control,
                preserve_source_timestamps: false,
            },
            external_audio.as_mut(),
        )
    };

    if result.is_ok() {
        logo_fade_plan.finish(timeline);
    }

    result
}

fn should_loop_input(source_is_live: bool, duration_seconds: Option<f64>) -> bool {
    !source_is_live && duration_seconds.is_some_and(|duration| duration > 0.0)
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
    mut external_audio: Option<&mut ExternalAudioInput>,
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
                preserve_source_timestamps: false,
            },
            external_audio.as_deref_mut(),
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
    pub(crate) preserve_source_timestamps: bool,
}

#[derive(Clone, Copy, Default)]
struct MediaFadePlan {
    video_end_pts: Option<i64>,
    audio_end_pts: Option<i64>,
    video_frames: i64,
    audio_samples: i64,
}

impl MediaFadePlan {
    fn for_trimmed_clip(
        requested_duration_us: Option<i64>,
        seek_us: i64,
        source_duration_us: Option<i64>,
        timeline: &Timeline,
        cfg: &OutputConfig,
    ) -> Self {
        let Some(requested_duration_us) = requested_duration_us else {
            return Self::default();
        };
        let Some(source_duration_us) = source_duration_us else {
            return Self::default();
        };

        let scheduled_out_us = seek_us.saturating_add(requested_duration_us);
        if (scheduled_out_us - source_duration_us).abs() <= MEDIA_DURATION_TOLERANCE_US {
            return Self::default();
        }

        Self {
            video_end_pts: Some(
                timeline.video_pts
                    + div_ceil(
                        i128::from(requested_duration_us) * i128::from(cfg.fps),
                        1_000_000,
                    ) as i64,
            ),
            audio_end_pts: Some(
                timeline.audio_pts
                    + div_ceil(
                        i128::from(requested_duration_us) * i128::from(cfg.sample_rate),
                        1_000_000,
                    ) as i64,
            ),
            video_frames: (MEDIA_FADE_SECONDS * f64::from(cfg.fps)).round().max(1.0) as i64,
            audio_samples: (MEDIA_FADE_SECONDS * f64::from(cfg.sample_rate))
                .round()
                .max(1.0) as i64,
        }
    }

    fn video_opacity_at(self, pts: i64) -> f32 {
        self.video_end_pts.map_or(1.0, |end_pts| {
            ((end_pts - pts).max(0) as f64 / self.video_frames as f64).clamp(0.0, 1.0) as f32
        })
    }

    fn audio_gain_at(self, pts: i64) -> f32 {
        self.audio_end_pts.map_or(1.0, |end_pts| {
            ((end_pts - pts).max(0) as f64 / self.audio_samples as f64).clamp(0.0, 1.0) as f32
        })
    }

    fn for_external_audio_trim(
        requested_duration_us: Option<i64>,
        seek_us: i64,
        source_duration_us: Option<i64>,
        timeline: &Timeline,
        cfg: &OutputConfig,
    ) -> Self {
        let (Some(requested_duration_us), Some(source_duration_us)) =
            (requested_duration_us, source_duration_us)
        else {
            return Self::default();
        };

        let scheduled_out_us = seek_us.saturating_add(requested_duration_us);
        if scheduled_out_us.saturating_add(MEDIA_DURATION_TOLERANCE_US) >= source_duration_us {
            return Self::default();
        }

        Self {
            audio_end_pts: Some(
                timeline.audio_pts
                    + div_ceil(
                        i128::from(requested_duration_us) * i128::from(cfg.sample_rate),
                        1_000_000,
                    ) as i64,
            ),
            audio_samples: (MEDIA_FADE_SECONDS * f64::from(cfg.sample_rate))
                .round()
                .max(1.0) as i64,
            ..Self::default()
        }
    }
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
    mut external_audio: Option<&mut ExternalAudioInput>,
) -> Result<()> {
    timeline.source_timestamp_mode = options.preserve_source_timestamps;
    let seek_seconds = (!is_live_input(label))
        .then_some(options.seek_seconds)
        .flatten();
    let seek_us = seek_seconds.map(seconds_to_microseconds).unwrap_or(0);
    let (has_video, embedded_audio, video_has_invalid_time_base, audio_has_invalid_time_base) = {
        let streams = ictx.streams();
        let video_stream = streams.best(media::Type::Video);
        let audio_stream = streams.best(media::Type::Audio);
        let video_has_invalid_time_base = video_stream
            .as_ref()
            .is_some_and(|stream| !has_valid_time_base(stream.time_base()));
        let audio_has_invalid_time_base = audio_stream
            .as_ref()
            .is_some_and(|stream| !has_valid_time_base(stream.time_base()));
        (
            video_stream.is_some(),
            audio_stream.is_some(),
            video_has_invalid_time_base,
            audio_has_invalid_time_base,
        )
    };
    let has_audio = embedded_audio || external_audio.is_some();
    // An external track replaces the embedded one, so only its own decoder
    // needs a valid audio time base. Do not reject a valid video because an
    // unused embedded audio stream is malformed.
    let has_invalid_time_base =
        video_has_invalid_time_base || (external_audio.is_none() && audio_has_invalid_time_base);
    if !has_video && !has_audio {
        return Err(anyhow!("{label} contains no audio or video stream"));
    }

    if let Some(seek_seconds) = seek_seconds {
        if has_invalid_time_base {
            return Err(anyhow!(
                "cannot seek {label}: its selected stream has an invalid time base"
            ));
        }
        seek_input(&mut ictx, seek_seconds)?;
    }

    let video_stream = ictx.streams().best(media::Type::Video);
    let audio_stream = ictx.streams().best(media::Type::Audio);

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
    let audio_duration_us = audio_stream.as_ref().and_then(stream_duration_us);
    let video_end_pts = video_duration_us.map(|duration_us| {
        let duration_us = duration_us.saturating_sub(seek_us);
        timeline.video_pts
            + div_ceil(i128::from(duration_us) * i128::from(cfg.fps), 1_000_000) as i64
    });
    let audio_end_pts = (external_audio.is_none())
        .then_some(audio_duration_us)
        .flatten()
        .map(|duration_us| {
            let duration_us = duration_us.saturating_sub(seek_us);
            timeline.audio_pts
                + div_ceil(
                    i128::from(duration_us) * i128::from(cfg.sample_rate),
                    1_000_000,
                ) as i64
        });
    let audio_padding_end_pts = match (video_duration_us, audio_duration_us) {
        (Some(video_duration), Some(audio_duration))
            if audio_duration.saturating_add(MEDIA_DURATION_TOLERANCE_US) < video_duration =>
        {
            audio_end_pts
        }
        _ => None,
    };
    let video_padding_end_pts = if !has_video && embedded_audio {
        Some(timeline.video_pts)
    } else {
        match (video_duration_us, audio_duration_us) {
            (Some(video_duration), Some(audio_duration))
                if video_duration.saturating_add(MEDIA_DURATION_TOLERANCE_US) < audio_duration =>
            {
                video_end_pts
            }
            _ => None,
        }
    };
    let logo_fade_plan = options
        .logo_fade_plan
        .with_end_pts(video_limit_pts.or(video_end_pts));
    let media_fade_plan = MediaFadePlan::for_trimmed_clip(
        duration_us,
        seek_us,
        video_duration_us.or(audio_duration_us),
        timeline,
        cfg,
    );

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
    let mut audio = if external_audio.is_none() {
        match audio_stream {
            Some(ref stream) => Some(AudioDecoder::new(stream, cfg, label, trim_start_us)?),
            None => None,
        }
    } else {
        None
    };

    let video_index = video_stream.as_ref().map(format::stream::Stream::index);
    let audio_index = audio
        .as_ref()
        .and_then(|_| audio_stream.as_ref().map(format::stream::Stream::index));
    let mut video_finished = video.is_none();
    let video_finished_notified = video_finished;
    let mut decoded_video_frames = 0_i64;
    let mut decoded_audio_samples = 0_i64;
    output.set_video_end(video_end_pts)?;
    output.clear_vtt_subtitles()?;
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
        output.video_decoded()?;
        output.video_finished()?;
    }

    let result = (|| -> Result<()> {
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
                        media_fade_plan,
                        options.playback_control,
                    )?;
                    if !has_audio && !timeline.source_timestamp_mode {
                        synchronize_silence_to_video(
                            cfg,
                            timeline,
                            output,
                            options.playback_control,
                        )?;
                    } else if let Some(external_audio) = external_audio.as_deref_mut() {
                        external_audio.decode_until(
                            cfg,
                            timeline,
                            output,
                            Some(audio_pts_for_video_position(cfg, timeline, audio_limit_pts)),
                            &mut decoded_audio_samples,
                            options.playback_control,
                        )?;
                    }
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
                    media_fade_plan,
                    options.playback_control,
                )?;
            }

            // av_read_frame reports EOF for the whole input, not for each
            // stream. Use reliable per-stream durations to start padding as
            // soon as one embedded track reaches its declared end. Waiting
            // for container EOF can otherwise enqueue minutes of old audio or
            // video timestamps after the other stream has already advanced.
            if external_audio.is_none() && !timeline.source_timestamp_mode {
                let last_video_frame = video
                    .as_ref()
                    .and_then(|video| video.last_composited_frame.as_ref());
                synchronize_declared_stream_ends(
                    cfg,
                    timeline,
                    output,
                    video_padding_end_pts,
                    audio_padding_end_pts,
                    last_video_frame,
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
            cfg,
            &mut video,
            timeline,
            output,
            &mut decoded_video_frames,
            &mut decoded_audio_samples,
            &mut video_finished,
            video_limit_pts,
            logo_fade_plan,
            media_fade_plan,
            options.playback_control,
            !has_audio && !timeline.source_timestamp_mode,
            external_audio.as_deref_mut(),
        )?;
        if !has_audio && !timeline.source_timestamp_mode {
            synchronize_silence_to_video(cfg, timeline, output, options.playback_control)?;
        }
        if let Some(audio) = audio.as_mut() {
            benchmark::measure(Stage::AudioDecode, || audio.decoder.send_eof())?;
            receive_audio_frames(
                audio,
                timeline,
                output,
                &mut decoded_audio_samples,
                audio_limit_pts,
                media_fade_plan,
                options.playback_control,
            )?;
            flush_audio_resampler(
                audio,
                timeline,
                output,
                &mut decoded_audio_samples,
                audio_limit_pts,
                media_fade_plan,
            )?;
        } else if let Some(external_audio) = external_audio.as_deref_mut() {
            let target_audio_pts = if has_video {
                Some(audio_pts_for_video_position(cfg, timeline, audio_limit_pts))
            } else {
                audio_limit_pts
            };
            external_audio.decode_until(
                cfg,
                timeline,
                output,
                target_audio_pts,
                &mut decoded_audio_samples,
                options.playback_control,
            )?;
        }

        if decoded_video_frames == 0 && decoded_audio_samples == 0 {
            return Err(anyhow!(
                "{label} produced no decodable audio or video frames"
            ));
        }

        let last_video_frame = video
            .as_ref()
            .and_then(|video| video.last_composited_frame.as_ref());
        // The live receiver aligns the two source tracks when it takes over.
        // Applying file-style end padding here would reintroduce synthetic
        // timeline PTS into a stream whose source PTS are intentionally kept.
        if !timeline.source_timestamp_mode {
            synchronize_timeline(cfg, timeline, output, last_video_frame)?;
        }
        if !video_finished_notified {
            output.video_finished()?;
        }
        Ok(())
    })();

    if let Err(error) = &result
        && error.downcast_ref::<PlaybackSkipped>().is_some()
    {
        let last_video_frame = video
            .as_ref()
            .and_then(|video| video.last_composited_frame.as_ref());
        synchronize_after_skip(cfg, timeline, output, last_video_frame)?;
    }

    result
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
    cfg: &OutputConfig,
    video: &mut Option<VideoDecoder>,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    decoded_audio_samples: &mut i64,
    finished: &mut bool,
    limit_pts: Option<i64>,
    logo_fade_plan: LogoFadePlan,
    media_fade_plan: MediaFadePlan,
    playback_control: &PlaybackControl,
    synthesize_silence: bool,
    mut external_audio: Option<&mut ExternalAudioInput>,
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
            media_fade_plan,
            playback_control,
        )?;
        if synthesize_silence {
            synchronize_silence_to_video(cfg, timeline, output, playback_control)?;
        } else if let Some(external_audio) = external_audio.as_deref_mut() {
            external_audio.decode_until(
                cfg,
                timeline,
                output,
                Some(audio_pts_for_video_position(cfg, timeline, None)),
                decoded_audio_samples,
                playback_control,
            )?;
        }
    }
    repeat_single_video_frame_to_limit(
        cfg,
        video,
        timeline,
        output,
        decoded_frames,
        decoded_audio_samples,
        limit_pts,
        logo_fade_plan,
        media_fade_plan,
        playback_control,
        synthesize_silence,
        external_audio,
    )?;
    output.video_decoded()?;
    *finished = true;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn repeat_single_video_frame_to_limit<O: FrameOutput>(
    cfg: &OutputConfig,
    video: &mut Option<VideoDecoder>,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    decoded_audio_samples: &mut i64,
    limit_pts: Option<i64>,
    logo_fade_plan: LogoFadePlan,
    media_fade_plan: MediaFadePlan,
    playback_control: &PlaybackControl,
    synthesize_silence: bool,
    mut external_audio: Option<&mut ExternalAudioInput>,
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
    let Some(frame) = video.last_output_frame.as_ref() else {
        return Ok(());
    };
    let pristine = reference_video_frame(frame)?;

    debug!(
        "holding single decoded video frame for {repeat_frames} frame(s) ({:.6} s)",
        repeat_frames as f64 / f64::from(video.output_fps)
    );

    while timeline.video_pts < limit_pts {
        check_playback_control(playback_control)?;
        let frame = reference_video_frame(&pristine)?;
        encode_composited_frame(
            frame,
            video,
            timeline,
            media_fade_plan,
            logo_fade_plan,
            output,
            decoded_frames,
            None,
        )?;
        if synthesize_silence {
            synchronize_silence_to_video(cfg, timeline, output, playback_control)?;
        } else if let Some(external_audio) = external_audio.as_deref_mut() {
            external_audio.decode_until(
                cfg,
                timeline,
                output,
                Some(audio_pts_for_video_position(cfg, timeline, None)),
                decoded_audio_samples,
                playback_control,
            )?;
        }
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

/// Returns the audio position that matches the current video frame. An
/// external audio track must only be decoded this far while video advances;
/// decoding it up to the clip end first lets desktop audio outrun the image.
fn audio_pts_for_video_position(
    cfg: &OutputConfig,
    timeline: &Timeline,
    limit_pts: Option<i64>,
) -> i64 {
    let position = div_ceil(
        i128::from(timeline.video_pts) * i128::from(cfg.sample_rate),
        i128::from(cfg.fps),
    ) as i64;
    limit_pts.map_or(position, |limit| position.min(limit))
}

fn stream_duration_us(stream: &format::stream::Stream) -> Option<i64> {
    if stream.duration() > 0 && has_valid_time_base(stream.time_base()) {
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

#[allow(clippy::too_many_arguments)]
fn receive_video_frames<O: FrameOutput>(
    video: &mut VideoDecoder,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    limit_pts: Option<i64>,
    logo_fade_plan: LogoFadePlan,
    media_fade_plan: MediaFadePlan,
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

        let source_pts = raw.timestamp().or_else(|| raw.pts()).map(|pts| {
            pts.rescale(
                video.frame_rate_converter.input_time_base,
                Rational(1, video.output_fps as i32),
            )
        });
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
        // source. Keep its pristine buffer referenced; copy-on-write below
        // allocates another buffer only if compositing changes its pixels.
        if *decoded_frames == 0 {
            video.last_output_frame = Some(reference_video_frame(&pristine)?);
        }
        if output_frames == 1 {
            // Pass the scaler result directly. The compositing helper keeps
            // it shared when possible and handles copy-on-write when the
            // single-frame repeat source or another owner aliases it.
            check_playback_control(playback_control)?;
            if limit_pts.is_some_and(|limit| timeline.video_pts >= limit) {
                return Ok(());
            }
            encode_composited_frame(
                pristine,
                video,
                timeline,
                media_fade_plan,
                logo_fade_plan,
                output,
                decoded_frames,
                source_pts,
            )?;
        } else {
            // Frame-rate up-conversion can share the pristine pixels. The
            // compositing helper requests a writable buffer only when an
            // active effect actually changes them.
            for index in 0..output_frames {
                check_playback_control(playback_control)?;
                if limit_pts.is_some_and(|limit| timeline.video_pts >= limit) {
                    return Ok(());
                }
                let frame = reference_video_frame(&pristine)?;
                encode_composited_frame(
                    frame,
                    video,
                    timeline,
                    media_fade_plan,
                    logo_fade_plan,
                    output,
                    decoded_frames,
                    source_pts.map(|pts| pts - (output_frames - 1 - index)),
                )?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_composited_frame<O: FrameOutput>(
    mut frame: frame::Video,
    video: &mut VideoDecoder,
    timeline: &mut Timeline,
    media_fade_plan: MediaFadePlan,
    logo_fade_plan: LogoFadePlan,
    output: &mut O,
    decoded_frames: &mut i64,
    source_pts: Option<i64>,
) -> Result<()> {
    video.update_runtime_text(timeline.video_pts, timeline.text_pts);
    let video_opacity = media_fade_plan.video_opacity_at(timeline.video_pts);
    let changes_pixels = video_frame_needs_write(
        video_opacity,
        video.logo.is_some(),
        video.text.is_some(),
        video.runtime_text.is_some(),
    );
    if changes_pixels {
        make_video_frame_writable(&mut frame)?;
    }
    apply_video_fade(&mut frame, video_opacity);
    apply_overlays(&mut frame, video, timeline, logo_fade_plan, output);
    frame.set_pts(Some(if timeline.source_timestamp_mode {
        source_pts.unwrap_or(timeline.video_pts)
    } else {
        timeline.video_pts
    }));
    output.encode_video(&frame)?;
    let output_position_ms = frame
        .pts()
        .unwrap_or(timeline.video_pts)
        .saturating_mul(1_000)
        / i64::from(video.output_fps);
    output.advance_vtt_subtitles(output_position_ms)?;
    video.last_composited_frame = Some(frame);
    timeline.video_pts += 1;
    *decoded_frames += 1;
    Ok(())
}

fn video_frame_needs_write(
    opacity: f32,
    has_logo: bool,
    has_static_text: bool,
    has_runtime_text: bool,
) -> bool {
    opacity < 1.0 || has_logo || has_static_text || has_runtime_text
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
    if let Some(text) = &mut video.runtime_text {
        let (width, height) = text.dimensions();
        benchmark::measure_overlay(Stage::TextRuntime, width, height, || {
            text.blend(frame, timeline.video_pts, timeline.text_pts);
        });
    }
    timeline.text_pts += 1;
}

/// Fades the decoded video toward limited-range YUV black before overlays are
/// blended, so logos and text remain fully visible during a clipped end.
fn apply_video_fade(frame: &mut frame::Video, opacity: f32) {
    if opacity >= 1.0 {
        return;
    }

    for plane in 0..frame.planes() {
        let target = if plane == 0 { 16.0 } else { 128.0 };
        let width = if plane == 0 {
            frame.width()
        } else {
            frame.width() / 2
        } as usize;
        let height = if plane == 0 {
            frame.height()
        } else {
            frame.height() / 2
        } as usize;
        let stride = frame.stride(plane);
        let data = frame.data_mut(plane);
        for row in data.chunks_mut(stride).take(height) {
            for sample in &mut row[..width] {
                *sample = (*sample as f32 * opacity + target * (1.0 - opacity)).round() as u8;
            }
        }
    }
}

fn apply_audio_fade(frame: &mut frame::Audio, start_pts: i64, fade: MediaFadePlan) {
    if fade.audio_end_pts.is_none() {
        return;
    }

    for plane in 0..frame.planes() {
        for (sample_index, sample) in frame.plane_mut::<f32>(plane).iter_mut().enumerate() {
            *sample *= fade.audio_gain_at(start_pts + sample_index as i64);
        }
    }
}

fn receive_audio_frames<O: FrameOutput>(
    audio: &mut AudioDecoder,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_samples: &mut i64,
    limit_pts: Option<i64>,
    media_fade_plan: MediaFadePlan,
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

        let mut converted = benchmark::measure(Stage::AudioProcess, || {
            resample_audio_frame(&mut audio.resampler, &raw)
        })?;
        let source_pts = raw
            .timestamp()
            .or_else(|| raw.pts())
            .map(|pts| pts.rescale(audio.input_time_base, Rational(1, converted.rate() as i32)));
        let samples = converted.samples() as i64;
        apply_audio_fade(&mut converted, timeline.audio_pts, media_fade_plan);
        converted.set_pts(Some(if timeline.source_timestamp_mode {
            source_pts
                .or(audio.source_next_pts)
                .unwrap_or(timeline.audio_pts)
        } else {
            timeline.audio_pts
        }));
        output.encode_audio(&converted)?;
        audio.source_next_pts = converted.pts().map(|pts| pts + samples);
        timeline.audio_pts += samples;
        *decoded_samples += samples;
    }
    Ok(())
}

fn resample_audio_frame(
    resampler: &mut resampling::Context,
    input: &frame::Audio,
) -> Result<frame::Audio> {
    let input_samples = i32::try_from(input.samples()).context("audio frame is too large")?;
    // SAFETY: the context is initialized and owned by `resampler`; the input
    // sample count was checked to fit FFmpeg's API type above.
    let output_capacity =
        unsafe { ffmpeg_next::ffi::swr_get_out_samples(resampler.as_mut_ptr(), input_samples) };
    if output_capacity < 0 {
        return Err(ffmpeg_next::Error::from(output_capacity))
            .context("calculating audio resampler output capacity");
    }

    let output = *resampler.output();
    let mut converted = frame::Audio::new(
        output.format,
        output_capacity.max(1) as usize,
        output.channel_layout,
    );
    converted.set_rate(output.rate);
    resampler.run(input, &mut converted)?;
    Ok(converted)
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
    media_fade_plan: MediaFadePlan,
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

        apply_audio_fade(&mut converted, timeline.audio_pts, media_fade_plan);
        converted.set_pts(Some(if timeline.source_timestamp_mode {
            audio.source_next_pts.unwrap_or(timeline.audio_pts)
        } else {
            timeline.audio_pts
        }));
        output.encode_audio(&converted)?;
        audio.source_next_pts = converted.pts().map(|pts| pts + samples);
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

fn has_valid_time_base(time_base: Rational) -> bool {
    time_base.numerator() > 0 && time_base.denominator() > 0
}

fn has_valid_frame_rate(frame_rate: Rational) -> bool {
    frame_rate.numerator() > 0 && frame_rate.denominator() > 0
}

fn fallback_video_time_base(
    average_frame_rate: Rational,
    frame_rate: Rational,
    output_fps: u32,
) -> Result<Rational> {
    let frame_rate = [average_frame_rate, frame_rate]
        .into_iter()
        .find(|frame_rate| has_valid_frame_rate(*frame_rate))
        .unwrap_or(Rational(
            i32::try_from(output_fps).context("output frame rate exceeds FFmpeg limits")?,
            1,
        ));

    Ok(Rational(frame_rate.denominator(), frame_rate.numerator()))
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
        let stream_time_base = stream.time_base();
        let timestamps_reliable = has_valid_time_base(stream_time_base);
        let input_time_base = if timestamps_reliable {
            stream_time_base
        } else {
            let fallback =
                fallback_video_time_base(stream.avg_frame_rate(), stream.rate(), cfg.fps)?;
            warn!(
                "{label}: video stream has invalid time base {stream_time_base}; using {fallback} derived from frame rate"
            );
            fallback
        };
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
            frame_rate_converter: FrameRateConverter::new(
                input_time_base,
                cfg.fps,
                timestamps_reliable,
            ),
            output_fps: cfg.fps,
            trim_start_us: timestamps_reliable.then_some(trim_start_us).flatten(),
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
    timestamps_reliable: bool,
    first_timestamp: Option<i64>,
    next_output_frame: i64,
    next_synthetic_timestamp: i64,
}

impl FrameRateConverter {
    fn new(input_time_base: Rational, output_fps: u32, timestamps_reliable: bool) -> Self {
        Self {
            input_time_base,
            output_time_base: Rational(1, output_fps as i32),
            timestamps_reliable,
            first_timestamp: None,
            next_output_frame: 0,
            next_synthetic_timestamp: 0,
        }
    }

    fn output_frames(&mut self, timestamp: Option<i64>) -> i64 {
        let timestamp = if self.timestamps_reliable {
            timestamp
        } else {
            let timestamp = self.next_synthetic_timestamp;
            self.next_synthetic_timestamp += 1;
            Some(timestamp)
        };
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
    source_next_pts: Option<i64>,
    decoder: codec::decoder::Audio,
    resampler: resampling::Context,
    input_channel_layout: ChannelLayout,
    input_time_base: Rational,
    trim_start_us: Option<i64>,
}

/// A second input used as a clip's external audio track. It owns its demuxer
/// so packets can be read incrementally in lockstep with the video timeline.
pub(crate) struct ExternalAudioInput {
    input: format::context::Input,
    stream_index: usize,
    fade_plan: MediaFadePlan,
    decoder: AudioDecoder,
    eof: bool,
}

impl ExternalAudioInput {
    fn open(
        path: &str,
        cfg: &OutputConfig,
        timeline: &Timeline,
        seek_seconds: Option<f64>,
        duration_seconds: Option<f64>,
    ) -> Result<Self> {
        let mut input = open_media_input(path)?;
        let container_duration_us = (input.duration() > 0).then_some(input.duration());
        let seek_seconds = (!is_live_input(path)).then_some(seek_seconds).flatten();
        if let Some(seek_seconds) = seek_seconds {
            seek_input(&mut input, seek_seconds)
                .with_context(|| format!("failed to seek external audio {path}"))?;
        }
        let stream = input
            .streams()
            .best(media::Type::Audio)
            .ok_or_else(|| anyhow!("{path} contains no audio stream"))?;
        let stream_index = stream.index();
        let duration_us = stream_duration_us(&stream).or(container_duration_us);
        let trim_start_us = seek_seconds
            .map(seconds_to_microseconds)
            .filter(|seek| *seek > 0);
        let decoder = AudioDecoder::new(&stream, cfg, path, trim_start_us)?;
        let fade_plan = MediaFadePlan::for_external_audio_trim(
            duration_seconds.map(seconds_to_microseconds),
            trim_start_us.unwrap_or_default(),
            duration_us,
            timeline,
            cfg,
        );

        Ok(Self {
            input,
            stream_index,
            fade_plan,
            decoder,
            eof: false,
        })
    }

    fn decode_until<O: FrameOutput>(
        &mut self,
        cfg: &OutputConfig,
        timeline: &mut Timeline,
        output: &mut O,
        target_audio_pts: Option<i64>,
        decoded_samples: &mut i64,
        playback_control: &PlaybackControl,
    ) -> Result<()> {
        while !self.eof && target_audio_pts.is_none_or(|target| timeline.audio_pts < target) {
            check_playback_control(playback_control)?;
            let packet = {
                let mut packets = self.input.packets();
                loop {
                    match packets.next() {
                        Some((stream, packet)) if stream.index() == self.stream_index => {
                            break Some(packet);
                        }
                        Some(_) => {}
                        None => break None,
                    }
                }
            };

            let Some(packet) = packet else {
                self.eof = true;
                benchmark::measure(Stage::AudioDecode, || self.decoder.decoder.send_eof())?;
                receive_audio_frames(
                    &mut self.decoder,
                    timeline,
                    output,
                    decoded_samples,
                    target_audio_pts,
                    self.fade_plan,
                    playback_control,
                )?;
                flush_audio_resampler(
                    &mut self.decoder,
                    timeline,
                    output,
                    decoded_samples,
                    target_audio_pts,
                    self.fade_plan,
                )?;
                break;
            };

            benchmark::measure(Stage::AudioDecode, || {
                self.decoder.decoder.send_packet(&packet)
            })?;
            receive_audio_frames(
                &mut self.decoder,
                timeline,
                output,
                decoded_samples,
                target_audio_pts,
                self.fade_plan,
                playback_control,
            )?;
        }

        if self.eof
            && let Some(target_audio_pts) = target_audio_pts
        {
            let silence_samples = target_audio_pts.saturating_sub(timeline.audio_pts);
            if silence_samples > 0 {
                write_silence(cfg, timeline, output, silence_samples)?;
                *decoded_samples += silence_samples;
            }
        }

        Ok(())
    }
}

impl AudioDecoder {
    fn new(
        stream: &format::stream::Stream,
        cfg: &OutputConfig,
        label: &str,
        trim_start_us: Option<i64>,
    ) -> Result<Self> {
        let ctx = codec::context::Context::from_parameters(stream.parameters())?;
        let mut decoder = ctx.decoder().audio()?;
        let stream_time_base = stream.time_base();
        let timestamps_reliable = has_valid_time_base(stream_time_base);
        let input_time_base = if timestamps_reliable {
            stream_time_base
        } else {
            let sample_rate =
                i32::try_from(decoder.rate()).context("audio sample rate exceeds FFmpeg limits")?;
            if sample_rate <= 0 {
                return Err(anyhow!(
                    "{label}: audio stream has an invalid time base and no sample rate"
                ));
            }
            let fallback = Rational(1, sample_rate);
            warn!(
                "{label}: audio stream has invalid time base {stream_time_base}; using {fallback} derived from sample rate"
            );
            fallback
        };
        decoder.set_packet_time_base(input_time_base);
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
            source_next_pts: None,
            input_channel_layout: channel_layout,
            input_time_base,
            trim_start_us: timestamps_reliable.then_some(trim_start_us).flatten(),
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

/// Keep audio packets interleaved with video for video-only inputs. Deferring
/// all silence until EOF puts its timestamps behind already-muxed video and
/// can make a realtime muxer wait indefinitely for an interleavable packet.
fn synchronize_silence_to_video<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    playback_control: &PlaybackControl,
) -> Result<()> {
    check_playback_control(playback_control)?;
    let target_audio_pts = div_ceil(
        i128::from(timeline.video_pts) * i128::from(cfg.sample_rate),
        i128::from(cfg.fps),
    ) as i64;
    let samples = target_audio_pts.saturating_sub(timeline.audio_pts);
    if samples > 0 {
        write_silence(cfg, timeline, output, samples)?;
    }
    check_playback_control(playback_control)
}

/// Keep streams interleaved once one embedded track reaches its declared end.
/// FFmpeg only reports container EOF, so without this guard a short track is
/// padded all at once after packets from the longer track have already been
/// muxed far into the future.
fn synchronize_declared_stream_ends<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    video_end_pts: Option<i64>,
    audio_end_pts: Option<i64>,
    last_video_frame: Option<&frame::Video>,
    playback_control: &PlaybackControl,
) -> Result<()> {
    check_playback_control(playback_control)?;

    // Decoder priming and end padding can make the decoded sample count end
    // slightly before the container duration. Two codec frames cover AAC's
    // usual priming plus the final partial frame without masking a real gap.
    let audio_end_tolerance = (output.audio_frame_size().max(1) as i64).saturating_mul(2);
    let audio_end_reached = audio_end_pts.is_some_and(|end_pts| {
        timeline.audio_pts >= end_pts
            || (timeline.audio_pts >= end_pts.saturating_sub(audio_end_tolerance)
                && i128::from(timeline.video_pts) * i128::from(cfg.sample_rate)
                    >= i128::from(end_pts) * i128::from(cfg.fps))
    });
    if audio_end_reached {
        synchronize_silence_to_video(cfg, timeline, output, playback_control)?;
    }

    let video_end_reached = video_end_pts.is_some_and(|end_pts| {
        timeline.video_pts >= end_pts
            || (timeline.video_pts >= end_pts.saturating_sub(1)
                && i128::from(timeline.audio_pts) * i128::from(cfg.fps)
                    >= i128::from(end_pts) * i128::from(cfg.sample_rate))
    });
    if video_end_reached {
        let (video_frames, _) = padding_to_sync(
            timeline.video_pts,
            timeline.audio_pts,
            cfg.fps,
            cfg.sample_rate,
        )?;
        if video_frames > 0 {
            write_padding_video_frames(cfg, timeline, output, video_frames, last_video_frame)?;
        }
    }

    check_playback_control(playback_control)
}

fn synchronize_after_skip<O: FrameOutput>(
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
    let target_video_pts = timeline.video_pts + video_frames;
    let target_audio_pts = timeline.audio_pts + audio_samples;

    if output.reset_after_skip(target_video_pts, target_audio_pts)? {
        timeline.video_pts = target_video_pts;
        timeline.audio_pts = target_audio_pts;
        return Ok(());
    }

    synchronize_timeline(cfg, timeline, output, last_video_frame)
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
            // The frame is already fully composited. Padding only changes its
            // header PTS, so all duplicates can safely share the pixel buffer.
            let mut frame = reference_video_frame(last_video_frame)?;
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
    if samples > 0 && output.pad_audio(samples)? {
        timeline.audio_pts += samples;
        return Ok(());
    }

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
    use ffmpeg_next::{codec, frame, media, util::format::pixel::Pixel};

    use super::{
        AudioDecoder, FrameRateConverter, InputPlaybackOptions, LogoFade, LogoFadePlan,
        MediaFadePlan, PlaybackControl, Rational, Timeline, apply_audio_fade,
        fallback_video_time_base, fit_dimensions, has_valid_time_base, padding_to_sync,
        parse_duration_us, play_clip, play_clip_with_external_audio, play_opened_input,
        resample_audio_frame, should_loop_input, should_play_loop_iteration,
        single_frame_repeat_frames, synchronize_after_skip, synchronize_declared_stream_ends,
        video_frame_needs_write, write_padding_video_frames,
    };
    use crate::{
        output::FrameOutput,
        utils::{config::OutputConfig, helper::open_media_input},
    };

    #[derive(Default)]
    struct RecordingOutput {
        video_frames: Vec<(u32, u32, i64)>,
        video_data_ptrs: Vec<usize>,
        audio_samples: usize,
        audio_frame_samples: Vec<usize>,
        audio_pts: Vec<i64>,
        events: Vec<&'static str>,
        reset_on_skip: bool,
        skip_target: Option<(i64, i64)>,
    }

    #[test]
    fn live_transports_are_never_looped_to_fill_a_playlist_slot() {
        assert!(!should_loop_input(true, Some(60.0)));
        assert!(should_loop_input(false, Some(60.0)));
    }

    #[test]
    fn reanchoring_after_live_updates_all_persistent_timeline_clocks() {
        let mut timeline = Timeline::new();
        timeline.text_pts = 12;
        timeline.source_timestamp_mode = true;

        timeline.reanchor(250, 480_000);

        assert_eq!(timeline.video_pts, 250);
        assert_eq!(timeline.audio_pts, 480_000);
        assert_eq!(timeline.text_pts, 250);
        assert!(!timeline.source_timestamp_mode);
    }

    impl FrameOutput for RecordingOutput {
        fn audio_frame_size(&self) -> usize {
            1024
        }

        fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
            self.video_data_ptrs.push(frame.data(0).as_ptr() as usize);
            self.video_frames.push((
                frame.width(),
                frame.height(),
                frame.pts().unwrap_or_default(),
            ));
            self.events.push("video");
            Ok(())
        }

        fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
            self.audio_samples += frame.samples();
            self.audio_frame_samples.push(frame.samples());
            self.audio_pts.push(frame.pts().unwrap_or_default());
            self.events.push("audio");
            Ok(())
        }

        fn video_finished(&mut self) -> Result<()> {
            self.events.push("video_finished");
            Ok(())
        }

        fn reset_after_skip(&mut self, video_pts: i64, audio_pts: i64) -> Result<bool> {
            self.skip_target = Some((video_pts, audio_pts));
            Ok(self.reset_on_skip)
        }
    }

    #[test]
    fn video_frame_only_needs_write_for_active_pixel_effects() {
        assert!(!video_frame_needs_write(1.0, false, false, false));
        assert!(video_frame_needs_write(0.5, false, false, false));
        assert!(video_frame_needs_write(1.0, true, false, false));
        assert!(video_frame_needs_write(1.0, false, true, false));
        assert!(video_frame_needs_write(1.0, false, false, true));
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
    fn declared_audio_end_is_padded_while_video_continues() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline {
            video_pts: 26,
            audio_pts: 48_000,
            ..Timeline::new()
        };
        let mut output = RecordingOutput::default();

        synchronize_declared_stream_ends(
            &cfg,
            &mut timeline,
            &mut output,
            Some(250),
            Some(48_000),
            None,
            &PlaybackControl::default(),
        )
        .unwrap();

        assert_eq!(timeline.video_pts, 26);
        assert_eq!(timeline.audio_pts, 49_920);
        assert_eq!(output.audio_samples, 1_920);
        assert_eq!(output.events, ["audio", "audio"]);
    }

    #[test]
    fn declared_video_end_is_padded_while_audio_continues() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline {
            video_pts: 25,
            audio_pts: 49_920,
            ..Timeline::new()
        };
        let mut output = RecordingOutput::default();
        let last_frame = frame::Video::new(Pixel::YUV420P, 320, 240);

        synchronize_declared_stream_ends(
            &cfg,
            &mut timeline,
            &mut output,
            Some(25),
            Some(480_000),
            Some(&last_frame),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert_eq!(timeline.video_pts, 26);
        assert_eq!(timeline.audio_pts, 49_920);
        assert_eq!(output.video_frames.len(), 1);
        assert_eq!(output.video_frames[0].2, 25);
        assert_eq!(output.events, ["video"]);
    }

    #[test]
    fn video_padding_shares_composited_pixel_buffer() {
        let cfg = OutputConfig::new(4, 4, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();
        let frame = frame::Video::new(Pixel::YUV420P, 4, 4);
        let source_data = frame.data(0).as_ptr() as usize;

        write_padding_video_frames(&cfg, &mut timeline, &mut output, 3, Some(&frame)).unwrap();

        assert_eq!(output.video_data_ptrs, vec![source_data; 3]);
        assert_eq!(
            output
                .video_frames
                .iter()
                .map(|(_, _, pts)| *pts)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn skipped_encoded_clip_pads_the_shorter_timeline() {
        let cfg = OutputConfig::new(1280, 720, 25, 48_000);
        let mut timeline = Timeline {
            video_pts: 50,
            audio_pts: 95_000,
            ..Timeline::new()
        };
        let mut output = RecordingOutput::default();

        synchronize_after_skip(&cfg, &mut timeline, &mut output, None).unwrap();

        assert_eq!(timeline.video_pts, 50);
        assert_eq!(timeline.audio_pts, 96_000);
        assert_eq!(output.audio_samples, 1_000);
        assert_eq!(output.skip_target, Some((50, 96_000)));
    }

    #[test]
    fn skipped_desktop_clip_resets_queued_output_without_padding() {
        let cfg = OutputConfig::new(1280, 720, 25, 48_000);
        let mut timeline = Timeline {
            video_pts: 49,
            audio_pts: 96_000,
            ..Timeline::new()
        };
        let mut output = RecordingOutput {
            reset_on_skip: true,
            ..RecordingOutput::default()
        };

        synchronize_after_skip(&cfg, &mut timeline, &mut output, None).unwrap();

        assert_eq!(timeline.video_pts, 50);
        assert_eq!(timeline.audio_pts, 96_000);
        assert!(output.video_frames.is_empty());
        assert_eq!(output.audio_samples, 0);
        assert_eq!(output.skip_target, Some((50, 96_000)));
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
    fn upsampling_does_not_accumulate_resampler_delay() {
        use ffmpeg_next::{
            format::sample::{Sample, Type},
            software::resampling,
            util::channel_layout::ChannelLayout,
        };

        const INPUT_RATE: u32 = 44_100;
        const OUTPUT_RATE: u32 = 48_000;
        const FRAME_SAMPLES: usize = 1_024;
        const FRAME_COUNT: usize = 100;

        let format = Sample::F32(Type::Planar);
        let mut resampler = resampling::Context::get(
            format,
            ChannelLayout::STEREO,
            INPUT_RATE,
            format,
            ChannelLayout::STEREO,
            OUTPUT_RATE,
        )
        .unwrap();
        let mut input = frame::Audio::new(format, FRAME_SAMPLES, ChannelLayout::STEREO);
        input.set_rate(INPUT_RATE);
        let mut output_samples = 0_usize;

        for _ in 0..FRAME_COUNT {
            let output = resample_audio_frame(&mut resampler, &input).unwrap();
            assert!(output.samples() > FRAME_SAMPLES);
            output_samples += output.samples();
        }

        let expected = FRAME_SAMPLES * FRAME_COUNT * OUTPUT_RATE as usize / INPUT_RATE as usize;
        assert!(output_samples.abs_diff(expected) < 64);
        assert!(
            resampler.delay().is_none_or(|delay| delay.output < 64),
            "resampler retained too many output samples: {:?}",
            resampler.delay()
        );
    }

    #[test]
    fn resamples_44_1_khz_media_during_decode() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip(
            &media_mix_asset("av_sync.mp4"),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(1.0),
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert!(
            output
                .audio_frame_samples
                .iter()
                .any(|samples| *samples > 1_024)
        );
        assert!(
            (48_000..=50_000).contains(&output.audio_samples),
            "unexpected one-second audio output: {} samples",
            output.audio_samples
        );
    }

    #[test]
    fn source_timestamp_mode_keeps_audio_and_video_on_the_input_timebase() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let path = media_mix_asset("av_sync.mp4");
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();
        let control = PlaybackControl::default();
        let input = open_media_input(&path).unwrap();

        play_opened_input(
            &path,
            input,
            &cfg,
            &mut timeline,
            &mut output,
            InputPlaybackOptions {
                seek_seconds: Some(1.0),
                duration_seconds: Some(0.4),
                subtitles_media_path: None,
                logo_fade_plan: LogoFadePlan::none(0, &cfg),
                playback_control: &control,
                preserve_source_timestamps: true,
            },
            None,
        )
        .unwrap();

        assert!(output.video_frames.first().unwrap().2 >= 25);
        assert!(output.audio_pts.first().copied().unwrap_or_default() >= 48_000);
        assert!(
            output
                .video_frames
                .windows(2)
                .all(|frames| frames[1].2 > frames[0].2),
            "video source PTS must advance: {:?}",
            output.video_frames
        );
        assert!(output.audio_pts.windows(2).all(|pts| pts[1] > pts[0]));
    }

    #[test]
    fn audio_decoder_uses_stream_time_base_for_aac_padding() {
        let input = open_media_input(&media_mix_asset("av_sync.mp4")).unwrap();
        let stream = input.streams().best(media::Type::Audio).unwrap();
        let stream_time_base = stream.time_base();

        assert_eq!(stream.parameters().id(), codec::Id::AAC);

        // This reproduces the old decoder setup. FFmpeg cannot adjust AAC
        // priming or end-padding timestamps when this remains unset.
        let context = codec::context::Context::from_parameters(stream.parameters()).unwrap();
        let unconfigured_decoder = context.decoder().audio().unwrap();
        assert_ne!(unconfigured_decoder.packet_time_base(), stream_time_base);

        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let decoder = AudioDecoder::new(&stream, &cfg, "AAC test input", None).unwrap();
        assert_eq!(decoder.decoder.packet_time_base(), stream_time_base);
    }

    #[test]
    fn regression_live_resampler_tail_keeps_source_timestamps() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let path = media_mix_asset("av_sync.mp4");
        let input = open_media_input(&path).unwrap();
        let stream = input.streams().best(media::Type::Audio).unwrap();
        assert_eq!(
            codec::context::Context::from_parameters(stream.parameters())
                .unwrap()
                .decoder()
                .audio()
                .unwrap()
                .rate(),
            44_100
        );
        drop(input);
        let mut output = RecordingOutput::default();
        // Decode through EOF after seeking, leaving a real 44.1 -> 48 kHz
        // resampler tail. No synthetic frames or mock resampler are used.
        play_opened_input(
            &path,
            open_media_input(&path).unwrap(),
            &cfg,
            &mut Timeline::new(),
            &mut output,
            InputPlaybackOptions {
                seek_seconds: Some(1.0),
                duration_seconds: None,
                subtitles_media_path: None,
                logo_fade_plan: LogoFadePlan::none(0, &cfg),
                playback_control: &PlaybackControl::default(),
                preserve_source_timestamps: true,
            },
            None,
        )
        .unwrap();
        assert!(output.audio_pts.len() > 2);
        let count = output.audio_pts.len();
        assert!(
            output.audio_frame_samples[count - 1] < 64,
            "expected a resampler tail"
        );
        assert!(
            output.audio_pts[count - 1] > output.audio_pts[count - 2],
            "resampler tail PTS jumped backwards: {:?}",
            &output.audio_pts[count - 2..]
        );
    }

    #[test]
    fn detects_invalid_time_bases() {
        assert!(!has_valid_time_base(Rational(0, 0)));
        assert!(!has_valid_time_base(Rational(0, 1)));
        assert!(!has_valid_time_base(Rational(1, 0)));
        assert!(has_valid_time_base(Rational(1, 90_000)));
    }

    #[test]
    fn derives_video_time_base_from_source_frame_rate() {
        assert_eq!(
            fallback_video_time_base(Rational(30_000, 1_001), Rational(0, 0), 25).unwrap(),
            Rational(1_001, 30_000)
        );
        assert_eq!(
            fallback_video_time_base(Rational(0, 0), Rational(24, 1), 25).unwrap(),
            Rational(1, 24)
        );
        assert_eq!(
            fallback_video_time_base(Rational(0, 0), Rational(0, 0), 25).unwrap(),
            Rational(1, 25)
        );
    }

    #[test]
    fn invalid_video_time_base_uses_synthetic_frame_timestamps() {
        let mut converter = FrameRateConverter::new(Rational(1, 30), 25, false);
        let output_frames: i64 = (0..30).map(|_| converter.output_frames(None)).sum();

        assert_eq!(output_frames, 25);
    }

    #[test]
    fn media_fade_only_applies_when_scheduled_out_differs_from_source_duration() {
        let cfg = OutputConfig::new(320, 240, 25, 1_000);
        let timeline = Timeline::new();

        let full_clip =
            MediaFadePlan::for_trimmed_clip(Some(10_000_000), 0, Some(10_000_000), &timeline, &cfg);
        assert_eq!(full_clip.video_opacity_at(9 * 25), 1.0);

        let trimmed_clip =
            MediaFadePlan::for_trimmed_clip(Some(8_000_000), 0, Some(10_000_000), &timeline, &cfg);
        assert_eq!(trimmed_clip.video_opacity_at(7 * 25), 1.0);
        assert!(trimmed_clip.video_opacity_at(7 * 25 + 13) < 0.5);
        assert_eq!(trimmed_clip.video_opacity_at(8 * 25), 0.0);
    }

    #[test]
    fn media_fade_respects_seek_when_comparing_out_to_source_duration() {
        let cfg = OutputConfig::new(320, 240, 25, 1_000);
        let timeline = Timeline::new();
        let fade = MediaFadePlan::for_trimmed_clip(
            Some(8_000_000),
            2_000_000,
            Some(10_000_000),
            &timeline,
            &cfg,
        );

        assert_eq!(fade.video_opacity_at(7 * 25), 1.0);
    }

    #[test]
    fn external_audio_fades_when_the_playlist_cuts_it_short() {
        let cfg = OutputConfig::new(320, 240, 25, 1_000);
        let timeline = Timeline::new();
        let fade = MediaFadePlan::for_external_audio_trim(
            Some(2_000_000),
            0,
            Some(3_000_000),
            &timeline,
            &cfg,
        );

        assert_eq!(fade.video_opacity_at(49), 1.0);
        assert!(fade.audio_gain_at(1_500) < 0.6);
        assert_eq!(fade.audio_gain_at(2_000), 0.0);
    }

    #[test]
    fn external_audio_does_not_inherit_the_video_fade() {
        let cfg = OutputConfig::new(320, 240, 25, 1_000);
        let timeline = Timeline::new();
        let video_fade =
            MediaFadePlan::for_trimmed_clip(Some(2_000_000), 0, Some(3_000_000), &timeline, &cfg);
        let external_audio_fade = MediaFadePlan::for_external_audio_trim(
            Some(2_000_000),
            0,
            Some(2_000_000),
            &timeline,
            &cfg,
        );

        assert!(video_fade.video_opacity_at(49) < 0.1);
        assert_eq!(external_audio_fade.audio_gain_at(1_999), 1.0);
    }

    #[test]
    fn media_fade_attenuates_audio_samples_at_clip_end() {
        use ffmpeg_next::{
            format::sample::{Sample, Type},
            util::channel_layout::ChannelLayout,
        };

        let cfg = OutputConfig::new(320, 240, 25, 1_000);
        let fade = MediaFadePlan::for_trimmed_clip(
            Some(2_000_000),
            0,
            Some(3_000_000),
            &Timeline::new(),
            &cfg,
        );
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 4, ChannelLayout::STEREO);
        for plane in 0..audio.planes() {
            audio.plane_mut::<f32>(plane).fill(1.0);
        }

        apply_audio_fade(&mut audio, 1_998, fade);

        assert!(audio.plane::<f32>(0)[0] > 0.0);
        assert_eq!(audio.plane::<f32>(0)[2], 0.0);
        assert_eq!(audio.plane::<f32>(1)[3], 0.0);
    }

    #[test]
    fn finishes_video_only_after_padding_short_audio() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip(
            &media_mix_asset("short_audio.mp4"),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            None,
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert_eq!(timeline.video_pts, 250);
        assert_eq!(timeline.audio_pts, 480_000);
        let longest_video_run = output
            .events
            .split(|event| *event == "audio")
            .map(|events| events.iter().filter(|event| **event == "video").count())
            .max()
            .unwrap_or_default();
        assert!(
            longest_video_run <= 10,
            "short embedded audio must be followed by interleaved silence; longest video run was {longest_video_run}"
        );
        assert_eq!(output.events.last(), Some(&"video_finished"));
    }

    #[test]
    fn plays_video_without_audio_and_generates_silence() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip(
            &media_mix_asset("no_audio.mp4"),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            None,
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert!(
            timeline.video_pts > 0,
            "video-only input must advance video"
        );
        assert!(
            output.audio_samples > 0,
            "video-only input must produce silent output audio"
        );
        let first_audio = output
            .events
            .iter()
            .position(|event| *event == "audio")
            .expect("video-only input must emit audio frames");
        let video_finished = output
            .events
            .iter()
            .position(|event| *event == "video_finished")
            .expect("video-only input must finish video");
        assert!(
            first_audio < video_finished,
            "silence must be interleaved before the video stream finishes"
        );
        assert_eq!(output.events.last(), Some(&"video_finished"));
    }

    #[test]
    fn plays_audio_without_video_and_generates_interleaved_black_frames() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip(
            &media_mix_asset("audio.mp3"),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(1.0),
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert!((25..=26).contains(&timeline.video_pts));
        assert!((48_000..=50_000).contains(&timeline.audio_pts));
        let longest_audio_run = output
            .events
            .split(|event| *event == "video")
            .map(|events| events.iter().filter(|event| **event == "audio").count())
            .max()
            .unwrap_or_default();
        assert!(
            longest_audio_run <= 4,
            "audio-only input must interleave generated video; longest audio run was {longest_audio_run}"
        );
    }

    #[test]
    fn plays_external_audio_with_video_only_source() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip_with_external_audio(
            &media_mix_asset("no_audio.mp4"),
            Some(&media_mix_asset("audio.mp3")),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(1.0),
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert!((25..=26).contains(&timeline.video_pts));
        assert!((48_000..=50_000).contains(&timeline.audio_pts));
        assert!(output.audio_samples > 0);
        assert_eq!(output.events.last(), Some(&"video_finished"));
    }

    #[test]
    fn holds_an_image_while_playing_external_audio() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip_with_external_audio(
            &media_mix_asset("still.jpg"),
            Some(&media_mix_asset("audio.mp3")),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(1.0),
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert!((25..=26).contains(&timeline.video_pts));
        assert!((48_000..=50_000).contains(&timeline.audio_pts));
        assert!((25..=26).contains(&output.video_frames.len()));
        assert!(output.audio_samples > 0);
        assert!(
            output.events.iter().position(|event| *event == "video")
                < output.events.iter().position(|event| *event == "audio"),
            "the image frame must reach the output before the external audio advances"
        );
        assert_eq!(output.events.last(), Some(&"video_finished"));
    }

    #[test]
    fn short_external_audio_is_followed_by_interleaved_silence() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip_with_external_audio(
            &media_mix_asset("no_audio.mp4"),
            Some(&media_mix_asset("short_audio.mp4")),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(12.0),
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        let longest_video_run = output
            .events
            .split(|event| *event == "audio")
            .map(|events| events.iter().filter(|event| **event == "video").count())
            .max()
            .unwrap_or_default();
        assert!(
            longest_video_run < 10,
            "audio EOF must not leave a long run of video without audio packets"
        );
    }

    #[test]
    fn long_external_audio_does_not_prevent_video_looping() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip_with_external_audio(
            &media_mix_asset("short_video.mp4"),
            Some(&media_mix_asset("audio.mp3")),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(16.0),
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert!(
            output
                .events
                .iter()
                .filter(|event| **event == "video_finished")
                .count()
                >= 2,
            "the video source must be reopened instead of holding its final frame"
        );
        assert!((400..=402).contains(&timeline.video_pts));
    }

    #[test]
    fn holds_an_image_with_interleaved_silence() {
        let cfg = OutputConfig::new(320, 240, 25, 48_000);
        let mut timeline = Timeline::new();
        let mut output = RecordingOutput::default();

        play_clip(
            &media_mix_asset("still.jpg"),
            &cfg,
            &mut timeline,
            &mut output,
            None,
            Some(1.0),
            None,
            LogoFade::default(),
            &PlaybackControl::default(),
        )
        .unwrap();

        assert_eq!(timeline.video_pts, 25);
        assert_eq!(timeline.audio_pts, 48_000);
        let first_audio = output
            .events
            .iter()
            .position(|event| *event == "audio")
            .expect("image input must emit silence");
        let video_finished = output
            .events
            .iter()
            .position(|event| *event == "video_finished")
            .expect("image input must finish video");
        assert!(first_audio < video_finished);
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
        let mut converter = FrameRateConverter::new(Rational(1, 24), 25, true);
        let output_frames = (0..240)
            .map(|timestamp| converter.output_frames(Some(timestamp)))
            .sum::<i64>();

        assert_eq!(output_frames, 250);
    }

    #[test]
    fn converts_30_fps_to_25_fps() {
        let mut converter = FrameRateConverter::new(Rational(1, 30), 25, true);
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
