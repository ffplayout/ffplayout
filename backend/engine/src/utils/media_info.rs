use anyhow::Result;
use ffmpeg_next::{Rational, Rescale, codec, format, media};
use log::{error, info};

pub struct MediaInfo {
    pub duration_seconds: Option<f64>,
    pub fps: Option<f64>,
    pub resolution: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbe {
    pub format: ProbeFormat,
    pub audio: Vec<AudioStream>,
    pub video: Vec<VideoStream>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeFormat {
    pub duration: Option<f64>,
    pub nb_streams: i64,
    pub size: Option<i64>,
    pub bit_rate: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioStream {
    pub channels: Option<i64>,
    pub codec_name: Option<String>,
    pub duration: Option<f64>,
    pub sample_rate: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoStream {
    pub aspect_ratio: Option<String>,
    pub bit_rate: Option<i64>,
    pub codec_name: Option<String>,
    pub duration: Option<f64>,
    pub field_order: Option<String>,
    pub frame_rate: String,
    pub height: Option<i64>,
    pub nb_frames: Option<i64>,
    pub width: Option<i64>,
}

pub fn print_media_info(path: &str) {
    match probe_media_info(path) {
        Ok(info) => info!(
            "playing: {path} (length: {}, fps: {}, resolution: {})",
            format_duration(info.duration_seconds),
            format_fps(info.fps),
            format_resolution(info.resolution)
        ),
        Err(error) => error!("playing: {path} (metadata unavailable: {error})"),
    }
}

pub fn probe_media_info(path: &str) -> Result<MediaInfo> {
    let ictx = format::input(path)?;
    let duration_seconds = if ictx.duration() > 0 {
        Some(ictx.duration() as f64 / 1_000_000.0)
    } else {
        ictx.streams()
            .best(media::Type::Video)
            .and_then(|stream| stream_duration_seconds(&stream))
    };

    let Some(video_stream) = ictx.streams().best(media::Type::Video) else {
        return Ok(MediaInfo {
            duration_seconds,
            fps: None,
            resolution: None,
        });
    };

    let fps = rational_to_f64(video_stream.avg_frame_rate())
        .or_else(|| rational_to_f64(video_stream.rate()));
    let resolution = codec::context::Context::from_parameters(video_stream.parameters())
        .ok()
        .and_then(|context| context.decoder().video().ok())
        .map(|decoder| (decoder.width(), decoder.height()));

    Ok(MediaInfo {
        duration_seconds,
        fps,
        resolution,
    })
}

pub fn probe_media(path: &str) -> Result<MediaProbe> {
    let ictx = format::input(path)?;
    let format = ProbeFormat {
        duration: (ictx.duration() > 0).then_some(ictx.duration() as f64 / 1_000_000.0),
        nb_streams: ictx.nb_streams() as i64,
        size: None,
        bit_rate: (ictx.bit_rate() > 0).then_some(ictx.bit_rate()),
    };
    let mut audio = Vec::new();
    let mut video = Vec::new();

    for stream in ictx.streams() {
        match stream.parameters().medium() {
            media::Type::Audio => audio.push(probe_audio_stream(&stream)),
            media::Type::Video => video.push(probe_video_stream(&stream)),
            _ => {}
        }
    }

    Ok(MediaProbe {
        format,
        audio,
        video,
    })
}

fn probe_audio_stream(stream: &format::stream::Stream) -> AudioStream {
    let parameters = stream.parameters();
    let codec_name = codec::decoder::find(parameters.id()).map(|codec| codec.name().to_string());
    let mut result = AudioStream {
        channels: None,
        codec_name,
        duration: stream_duration_seconds(stream),
        sample_rate: None,
    };

    if let Ok(context) = codec::context::Context::from_parameters(parameters)
        && let Ok(decoder) = context.decoder().audio()
    {
        result.sample_rate = Some(i64::from(decoder.rate()));
        result.channels = Some(i64::from(decoder.channels()));
    }

    result
}

fn probe_video_stream(stream: &format::stream::Stream) -> VideoStream {
    let parameters = stream.parameters();
    let codec_name = codec::decoder::find(parameters.id()).map(|codec| codec.name().to_string());
    let mut result = VideoStream {
        aspect_ratio: None,
        bit_rate: None,
        codec_name,
        duration: stream_duration_seconds(stream),
        field_order: None,
        frame_rate: rational_string(stream.rate()),
        height: None,
        nb_frames: (stream.frames() > 0).then_some(stream.frames()),
        width: None,
    };

    if let Ok(context) = codec::context::Context::from_parameters(parameters)
        && let Ok(decoder) = context.decoder().video()
    {
        let width = decoder.width();
        let height = decoder.height();
        result.width = Some(i64::from(width));
        result.height = Some(i64::from(height));
        result.aspect_ratio = Some(format!("{width}:{height}"));
    }

    result
}

fn rational_string(value: Rational) -> String {
    if value.denominator() == 0 {
        "0/0".to_string()
    } else {
        format!("{}/{}", value.numerator(), value.denominator())
    }
}

fn stream_duration_seconds(stream: &format::stream::Stream) -> Option<f64> {
    if stream.duration() > 0 {
        let duration_us = stream
            .duration()
            .rescale(stream.time_base(), Rational(1, 1_000_000));
        return Some(duration_us as f64 / 1_000_000.0);
    }

    let metadata = stream.metadata();
    metadata
        .get("DURATION")
        .or_else(|| metadata.get("duration"))
        .and_then(parse_duration_seconds)
}

fn parse_duration_seconds(duration: &str) -> Option<f64> {
    let mut parts = duration.split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some(hours * 3_600.0 + minutes * 60.0 + seconds)
}

fn rational_to_f64(value: Rational) -> Option<f64> {
    if value.denominator() == 0 || value.numerator() <= 0 {
        return None;
    }

    Some(f64::from(value.numerator()) / f64::from(value.denominator()))
}

fn format_duration(duration_seconds: Option<f64>) -> String {
    let Some(duration_seconds) = duration_seconds else {
        return "unknown".to_string();
    };
    let total_millis = (duration_seconds * 1_000.0).round().max(0.0) as u64;
    let total_seconds = total_millis / 1_000;
    let millis = total_millis % 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{minutes:02}:{seconds:02}.{millis:03}")
    }
}

fn format_fps(fps: Option<f64>) -> String {
    fps.map(|fps| format!("{fps:.3}"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_resolution(resolution: Option<(u32, u32)>) -> String {
    resolution
        .map(|(width, height)| format!("{width}x{height}"))
        .unwrap_or_else(|| "unknown".to_string())
}
