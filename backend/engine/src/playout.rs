use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{
    Rational, Rescale, codec, format, frame, media,
    software::{resampling, scaling},
    util::{channel_layout::ChannelLayout, format::pixel::Pixel, format::sample::Sample},
};
use log::debug;

use crate::{
    output::FrameOutput,
    utils::config::{LogoConfig, OutputConfig},
};

#[derive(Clone, Copy)]
pub(crate) struct Timeline {
    video_pts: i64,
    audio_pts: i64,
}

impl Timeline {
    pub(crate) fn new() -> Self {
        Self {
            video_pts: 0,
            audio_pts: 0,
        }
    }
}

/// Plays one file into the continuous output timeline.
///
/// Input PTS are replaced with continuous timeline PTS. If only one media type
/// exists, the missing counterpart is synthesized.
pub(crate) fn play_clip<O: FrameOutput>(
    path: &str,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    seek_seconds: Option<f64>,
    duration_seconds: Option<f64>,
) -> Result<()> {
    if let Some(duration_seconds) = duration_seconds.filter(|duration| *duration > 0.0) {
        return play_looped_clip(path, cfg, timeline, output, seek_seconds, duration_seconds);
    }

    let ictx = format::input(path)?;
    play_opened_input(
        path,
        ictx,
        cfg,
        timeline,
        output,
        InputPlaybackOptions {
            seek_seconds,
            duration_seconds,
            subtitles_media_path: Some(path),
        },
    )
}

fn play_looped_clip<O: FrameOutput>(
    path: &str,
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    seek_seconds: Option<f64>,
    duration_seconds: f64,
) -> Result<()> {
    if !duration_seconds.is_finite() {
        return Err(anyhow!("clip duration must be a finite number"));
    }

    let mut remaining = duration_seconds;
    let mut first_iteration = true;
    let mut iterations = 0_u32;
    let minimum_progress = (1.0 / f64::from(cfg.fps)).min(1.0 / f64::from(cfg.sample_rate));

    while remaining > minimum_progress {
        let before_video_pts = timeline.video_pts;
        let before_audio_pts = timeline.audio_pts;
        let ictx = format::input(path)?;
        play_opened_input(
            path,
            ictx,
            cfg,
            timeline,
            output,
            InputPlaybackOptions {
                seek_seconds: first_iteration.then_some(seek_seconds).flatten(),
                duration_seconds: Some(remaining),
                subtitles_media_path: first_iteration.then_some(path),
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

        if remaining > minimum_progress {
            debug!(
                "looping {path} to fill requested duration; iteration {iterations}, remaining {:.6} s",
                remaining
            );
        }
    }

    Ok(())
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

    let trim_start_us = (seek_us > 0).then_some(seek_us);
    let mut video = match video_stream {
        Some(ref stream) => Some(VideoDecoder::new(stream, cfg, trim_start_us)?),
        None => None,
    };
    let mut audio = match audio_stream {
        Some(ref stream) => Some(AudioDecoder::new(stream, cfg, trim_start_us)?),
        None => None,
    };

    let video_index = video_stream.as_ref().map(format::stream::Stream::index);
    let audio_index = audio_stream.as_ref().map(format::stream::Stream::index);
    let video_duration_us = video_stream.as_ref().and_then(stream_duration_us);
    let mut video_finished = video.is_none();
    let mut decoded_video_frames = 0_i64;
    let mut decoded_audio_samples = 0_i64;
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

    let video_end_pts = video_duration_us.map(|duration_us| {
        let duration_us = duration_us.saturating_sub(seek_us);
        timeline.video_pts
            + div_ceil(i128::from(duration_us) * i128::from(cfg.fps), 1_000_000) as i64
    });
    output.set_video_end(video_end_pts)?;
    if let Some(media_path) = options.subtitles_media_path {
        output.write_vtt_subtitles(
            media_path,
            timeline.video_pts * 1_000 / i64::from(cfg.fps),
            seek_us / 1_000,
        )?;
    }

    if video_finished {
        output.video_finished()?;
    }

    for (stream, packet) in ictx.packets() {
        if Some(stream.index()) == video_index {
            if !video_finished && let Some(video) = video.as_mut() {
                video.decoder.send_packet(&packet)?;
                receive_video_frames(
                    video,
                    timeline,
                    output,
                    &mut decoded_video_frames,
                    video_limit_pts,
                )?;
            }
        } else if Some(stream.index()) == audio_index
            && let Some(audio) = audio.as_mut()
        {
            audio.decoder.send_packet(&packet)?;
            receive_audio_frames(
                audio,
                timeline,
                output,
                &mut decoded_audio_samples,
                audio_limit_pts,
            )?;
        }

        if video_limit_pts.is_none_or(|limit| timeline.video_pts >= limit)
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
    )?;
    if let Some(audio) = audio.as_mut() {
        audio.decoder.send_eof()?;
        receive_audio_frames(
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

    synchronize_timeline(cfg, timeline, output)
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

fn finish_video<O: FrameOutput>(
    video: &mut Option<VideoDecoder>,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    finished: &mut bool,
    limit_pts: Option<i64>,
) -> Result<()> {
    if *finished {
        return Ok(());
    }

    if let Some(video) = video.as_mut() {
        video.decoder.send_eof()?;
        receive_video_frames(video, timeline, output, decoded_frames, limit_pts)?;
    }
    repeat_single_video_frame_to_limit(video, timeline, output, decoded_frames, limit_pts)?;
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
        let mut frame = frame.clone();
        frame.set_pts(Some(timeline.video_pts));
        output.encode_video(&frame)?;
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

    Some(((hours * 3_600.0 + minutes * 60.0 + seconds) * 1_000_000.0).round() as i64)
}

fn receive_video_frames<O: FrameOutput>(
    video: &mut VideoDecoder,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_frames: &mut i64,
    limit_pts: Option<i64>,
) -> Result<()> {
    let mut raw = frame::Video::empty();
    while video.decoder.receive_frame(&mut raw).is_ok() {
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
        video.scaler.run(&raw, &mut scaled)?;
        let mut padded = if video.needs_padding() {
            let mut padded = black_video_frame(video.output_width, video.output_height);
            copy_video_frame(
                &scaled,
                &mut padded,
                video.x_offset,
                video.y_offset,
                video.scaled_width,
                video.scaled_height,
            );
            Some(padded)
        } else {
            None
        };
        for _ in 0..output_frames {
            if limit_pts.is_some_and(|limit| timeline.video_pts >= limit) {
                return Ok(());
            }

            let frame = padded.as_mut().unwrap_or(&mut scaled);
            if let Some(logo) = &video.logo {
                blend_logo(frame, logo);
            }
            frame.set_pts(Some(timeline.video_pts));
            video.last_output_frame = Some(frame.clone());
            output.encode_video(frame)?;
            timeline.video_pts += 1;
            *decoded_frames += 1;
        }
    }
    Ok(())
}

fn receive_audio_frames<O: FrameOutput>(
    audio: &mut AudioDecoder,
    timeline: &mut Timeline,
    output: &mut O,
    decoded_samples: &mut i64,
    limit_pts: Option<i64>,
) -> Result<()> {
    let mut raw = frame::Audio::empty();
    while audio.decoder.receive_frame(&mut raw).is_ok() {
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

        let mut converted = frame::Audio::empty();
        audio.resampler.run(&raw, &mut converted)?;
        apply_volume(&mut converted, audio.volume);
        let samples = converted.samples() as i64;
        converted.set_pts(Some(timeline.audio_pts));
        output.encode_audio(&converted)?;
        timeline.audio_pts += samples;
        *decoded_samples += samples;
    }
    Ok(())
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
    frame_rate_converter: FrameRateConverter,
    output_fps: u32,
    trim_start_us: Option<i64>,
    last_output_frame: Option<frame::Video>,
}

impl VideoDecoder {
    fn new(
        stream: &format::stream::Stream,
        cfg: &OutputConfig,
        trim_start_us: Option<i64>,
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
            frame_rate_converter: FrameRateConverter::new(stream.time_base(), cfg.fps),
            output_fps: cfg.fps,
            trim_start_us,
            last_output_frame: None,
        })
    }

    fn needs_padding(&self) -> bool {
        self.scaled_width != self.output_width
            || self.scaled_height != self.output_height
            || self.x_offset != 0
            || self.y_offset != 0
    }
}

struct LogoOverlay {
    frame: frame::Video,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    opacity: f64,
}

impl LogoOverlay {
    fn load(config: &LogoConfig, output_width: u32, output_height: u32) -> Result<Self> {
        if !(0.0..=1.0).contains(&config.opacity) || !config.opacity.is_finite() {
            return Err(anyhow!("logo opacity must be between 0.0 and 1.0"));
        }

        let mut ictx = format::input(&config.path)
            .with_context(|| format!("failed to open logo {}", config.path))?;
        let stream = ictx
            .streams()
            .best(media::Type::Video)
            .ok_or_else(|| anyhow!("logo {} contains no video/image stream", config.path))?;
        let stream_index = stream.index();
        let ctx = codec::context::Context::from_parameters(stream.parameters())?;
        let mut decoder = ctx.decoder().video()?;
        let input_width = decoder.width();
        let input_height = decoder.height();
        let (width, height) = logo_dimensions(
            config.scale.as_deref(),
            input_width,
            input_height,
            output_width,
            output_height,
        )?;
        let mut scaler = scaling::Context::get(
            decoder.format(),
            input_width,
            input_height,
            Pixel::RGBA,
            width,
            height,
            scaling::flag::Flags::BILINEAR,
        )?;

        let mut decoded = frame::Video::empty();
        let mut rgba = None;
        for (packet_stream, packet) in ictx.packets() {
            if packet_stream.index() != stream_index {
                continue;
            }
            decoder.send_packet(&packet)?;
            if decoder.receive_frame(&mut decoded).is_ok() {
                let mut scaled = frame::Video::empty();
                scaler.run(&decoded, &mut scaled)?;
                rgba = Some(scaled);
            }
            if rgba.is_some() {
                break;
            }
        }
        if rgba.is_none() {
            decoder.send_eof()?;
            if decoder.receive_frame(&mut decoded).is_ok() {
                let mut scaled = frame::Video::empty();
                scaler.run(&decoded, &mut scaled)?;
                rgba = Some(scaled);
            }
        }
        let frame = rgba.ok_or_else(|| anyhow!("logo {} produced no frame", config.path))?;
        let (x, y) = logo_position(&config.position, output_width, output_height, width, height)?;

        Ok(Self {
            frame,
            x,
            y,
            width,
            height,
            opacity: config.opacity,
        })
    }
}

fn logo_dimensions(
    scale: Option<&str>,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<(u32, u32)> {
    let Some(scale) = scale.filter(|scale| !scale.trim().is_empty()) else {
        return Ok((even(input_width).max(2), even(input_height).max(2)));
    };
    let (width, height) = scale
        .split_once(':')
        .or_else(|| scale.split_once('x'))
        .ok_or_else(|| anyhow!("logo scale must use WIDTH:HEIGHT or WIDTHxHEIGHT"))?;
    let width = parse_logo_dimension(width, input_width, output_width)?;
    let height = parse_logo_dimension(height, input_height, output_height)?;

    let (width, height) = match (width, height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (
            width,
            ((u64::from(width) * u64::from(input_height)) / u64::from(input_width)) as u32,
        ),
        (None, Some(height)) => (
            ((u64::from(height) * u64::from(input_width)) / u64::from(input_height)) as u32,
            height,
        ),
        (None, None) => (input_width, input_height),
    };

    Ok((even(width).max(2), even(height).max(2)))
}

fn parse_logo_dimension(value: &str, input: u32, output: u32) -> Result<Option<u32>> {
    let value = value.trim();
    if value == "-1" {
        return Ok(None);
    }
    if value == "iw" || value == "ih" {
        return Ok(Some(input));
    }
    if value == "W" || value == "H" || value == "main_w" || value == "main_h" {
        return Ok(Some(output));
    }
    value
        .parse::<u32>()
        .map(Some)
        .map_err(|_| anyhow!("unsupported logo scale expression {value:?}"))
}

fn logo_position(
    position: &str,
    output_width: u32,
    output_height: u32,
    logo_width: u32,
    logo_height: u32,
) -> Result<(u32, u32)> {
    let (x, y) = position
        .split_once(':')
        .ok_or_else(|| anyhow!("logo position must use X:Y"))?;
    let x = eval_position_expr(x, output_width, logo_width)?;
    let y = eval_position_expr(y, output_height, logo_height)?;
    Ok((
        x.clamp(0, i64::from(output_width.saturating_sub(logo_width))) as u32,
        y.clamp(0, i64::from(output_height.saturating_sub(logo_height))) as u32,
    ))
}

fn eval_position_expr(expr: &str, main: u32, overlay: u32) -> Result<i64> {
    let normalized = expr
        .replace("main_w", "M")
        .replace("main_h", "M")
        .replace("overlay_w", "O")
        .replace("overlay_h", "O")
        .replace(['W', 'H'], "M")
        .replace(['w', 'h'], "O")
        .replace('-', "+-");
    let mut total = 0_i64;
    for part in normalized.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (sign, part) = part
            .strip_prefix('-')
            .map_or((1_i64, part), |part| (-1_i64, part));
        let value = match part {
            "M" => i64::from(main),
            "O" => i64::from(overlay),
            _ => part
                .parse::<i64>()
                .map_err(|_| anyhow!("unsupported logo position expression {expr:?}"))?,
        };
        total += sign * value;
    }
    Ok(total)
}

fn blend_logo(target: &mut frame::Video, logo: &LogoOverlay) {
    let logo_data = logo.frame.data(0);
    let logo_stride = logo.frame.stride(0);
    let target_y_stride = target.stride(0);
    let target_u_stride = target.stride(1);
    let target_v_stride = target.stride(2);
    let target_y_len = target.data(0).len();
    let target_u_len = target.data(1).len();
    let target_v_len = target.data(2).len();

    for y in 0..logo.height as usize {
        for x in 0..logo.width as usize {
            let logo_index = y * logo_stride + x * 4;
            if logo_index + 3 >= logo_data.len() {
                continue;
            }
            let alpha = (f64::from(logo_data[logo_index + 3]) / 255.0) * logo.opacity;
            if alpha <= 0.0 {
                continue;
            }

            let target_x = logo.x as usize + x;
            let target_y = logo.y as usize + y;
            let y_index = target_y * target_y_stride + target_x;
            if y_index >= target_y_len {
                continue;
            }

            let uv_x = target_x / 2;
            let uv_y = target_y / 2;
            let u_index = uv_y * target_u_stride + uv_x;
            let v_index = uv_y * target_v_stride + uv_x;
            if u_index >= target_u_len || v_index >= target_v_len {
                continue;
            }

            let source_rgb = (
                f64::from(logo_data[logo_index]),
                f64::from(logo_data[logo_index + 1]),
                f64::from(logo_data[logo_index + 2]),
            );
            let target_rgb = yuv_to_rgb(
                target.data(0)[y_index],
                target.data(1)[u_index],
                target.data(2)[v_index],
            );
            let blended = (
                target_rgb.0 * (1.0 - alpha) + source_rgb.0 * alpha,
                target_rgb.1 * (1.0 - alpha) + source_rgb.1 * alpha,
                target_rgb.2 * (1.0 - alpha) + source_rgb.2 * alpha,
            );
            let (new_y, new_u, new_v) = rgb_to_yuv(blended);

            target.data_mut(0)[y_index] = new_y;
            target.data_mut(1)[u_index] = new_u;
            target.data_mut(2)[v_index] = new_v;
        }
    }
}

fn yuv_to_rgb(y: u8, u: u8, v: u8) -> (f64, f64, f64) {
    let y = f64::from(y) - 16.0;
    let u = f64::from(u) - 128.0;
    let v = f64::from(v) - 128.0;
    (
        (1.164 * y + 1.596 * v).clamp(0.0, 255.0),
        (1.164 * y - 0.392 * u - 0.813 * v).clamp(0.0, 255.0),
        (1.164 * y + 2.017 * u).clamp(0.0, 255.0),
    )
}

fn rgb_to_yuv((r, g, b): (f64, f64, f64)) -> (u8, u8, u8) {
    (
        (16.0 + 0.257 * r + 0.504 * g + 0.098 * b)
            .round()
            .clamp(0.0, 255.0) as u8,
        (128.0 - 0.148 * r - 0.291 * g + 0.439 * b)
            .round()
            .clamp(0.0, 255.0) as u8,
        (128.0 + 0.439 * r - 0.368 * g - 0.071 * b)
            .round()
            .clamp(0.0, 255.0) as u8,
    )
}

fn apply_volume(frame: &mut frame::Audio, volume: f64) {
    if (volume - 1.0).abs() <= f64::EPSILON {
        return;
    }
    let volume = volume as f32;
    for plane in 0..frame.planes() {
        for sample in frame.plane_mut::<f32>(plane) {
            *sample = if sample.is_finite() {
                *sample * volume
            } else {
                0.0
            };
        }
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

fn even(value: u32) -> u32 {
    value & !1
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
    input_time_base: Rational,
    trim_start_us: Option<i64>,
    volume: f64,
}

impl AudioDecoder {
    fn new(
        stream: &format::stream::Stream,
        cfg: &OutputConfig,
        trim_start_us: Option<i64>,
    ) -> Result<Self> {
        let ctx = codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = ctx.decoder().audio()?;
        let resampler = resampling::Context::get(
            decoder.format(),
            decoder.channel_layout(),
            decoder.rate(),
            Sample::F32(format::sample::Type::Planar),
            ChannelLayout::STEREO,
            cfg.sample_rate,
        )?;
        Ok(Self {
            decoder,
            resampler,
            input_time_base: stream.time_base(),
            trim_start_us,
            volume: cfg.volume,
        })
    }
}

pub(crate) fn write_fallback<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
    duration: f64,
) -> Result<()> {
    let video_end = timeline.video_pts + (duration * f64::from(cfg.fps)).ceil() as i64;
    let audio_end = timeline.audio_pts + (duration * f64::from(cfg.sample_rate)).ceil() as i64;

    while timeline.video_pts < video_end || timeline.audio_pts < audio_end {
        let video_time = timeline.video_pts as f64 / f64::from(cfg.fps);
        let audio_time = timeline.audio_pts as f64 / f64::from(cfg.sample_rate);

        if timeline.video_pts < video_end
            && (timeline.audio_pts >= audio_end || video_time <= audio_time)
        {
            write_black_frames(cfg, timeline, output, 1)?;
        } else {
            let remaining = (audio_end - timeline.audio_pts) as usize;
            let samples = remaining.min(output.audio_frame_size().max(1));
            write_silence_frame(cfg, timeline, output, samples)?;
        }
    }

    synchronize_timeline(cfg, timeline, output)
}

fn synchronize_timeline<O: FrameOutput>(
    cfg: &OutputConfig,
    timeline: &mut Timeline,
    output: &mut O,
) -> Result<()> {
    let (video_frames, audio_samples) = padding_to_sync(
        timeline.video_pts,
        timeline.audio_pts,
        cfg.fps,
        cfg.sample_rate,
    )?;

    if video_frames > 0 {
        debug!(
            "padding video with {video_frames} black frame(s) ({:.6} s) to synchronize the timeline",
            video_frames as f64 / f64::from(cfg.fps)
        );
    }
    write_black_frames(cfg, timeline, output, video_frames)?;

    if audio_samples > 0 {
        debug!(
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
) -> Result<()> {
    for _ in 0..frames {
        let mut black = black_video_frame_for_config(cfg);
        black.set_pts(Some(timeline.video_pts));
        output.encode_video(&black)?;
        timeline.video_pts += 1;
    }
    Ok(())
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
    use super::{
        FrameRateConverter, Rational, fit_dimensions, padding_to_sync, parse_duration_us,
        single_frame_repeat_frames,
    };

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
