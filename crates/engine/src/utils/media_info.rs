use std::sync::Arc;

use anyhow::Result;
use ffmpeg_next::{
    Rational, Rescale, codec, format, frame, media,
    software::resampling,
    util::{channel_layout::ChannelLayout, format::sample::Sample},
};
use log::{error, info};

use crate::utils::helper::{is_network_url, network_io_options, open_media_input};

const SILENCE_SAMPLE_RATE: u32 = 48_000;
const SILENCE_CHANNEL_LAYOUT: ChannelLayout = ChannelLayout::STEREO;

#[derive(Debug, Clone, PartialEq)]
pub struct SilenceDetection {
    pub silent: bool,
    pub analyzed_seconds: f64,
}

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
    let ictx = open_media_input(path)?;
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
    let parameters = video_stream.parameters();
    // SAFETY: stream parameters remain owned by the live input. Read only;
    // probing dimensions must not initialize a decoder and its worker pool.
    let resolution = unsafe {
        let parameters = &*parameters.as_ptr();
        (parameters.width > 0 && parameters.height > 0)
            .then_some((parameters.width as u32, parameters.height as u32))
    };

    Ok(MediaInfo {
        duration_seconds,
        fps,
        resolution,
    })
}

pub fn probe_media(path: &str) -> Result<MediaProbe> {
    let ictx = open_media_input(path)?;
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

pub fn detect_audio_silence(
    path: &str,
    seek_seconds: f64,
    duration_seconds: f64,
    threshold_db: f32,
    min_silence_seconds: f64,
) -> Result<SilenceDetection> {
    detect_audio_silence_with_cancel(
        path,
        seek_seconds,
        duration_seconds,
        threshold_db,
        min_silence_seconds,
        || false,
    )
}

/// Cancellation interrupts demuxer I/O and is checked between audio packets.
/// The callback must stay cheap; FFmpeg also invokes it while opening/seeking.
pub fn detect_audio_silence_with_cancel(
    path: &str,
    seek_seconds: f64,
    duration_seconds: f64,
    threshold_db: f32,
    min_silence_seconds: f64,
    cancelled: impl Fn() -> bool + Send + Sync + 'static,
) -> Result<SilenceDetection> {
    let cancelled = Arc::new(cancelled);
    let interrupt = cancelled.clone();

    if cancelled() {
        return Err(ffmpeg_next::Error::Exit.into());
    }

    let mut ictx = if is_network_url(path) {
        format::input_with_interrupt_and_dictionary(
            &path,
            move || interrupt(),
            network_io_options(),
        )?
    } else {
        format::input_with_interrupt(&path, move || interrupt())?
    };
    let stream = ictx
        .streams()
        .best(media::Type::Audio)
        .ok_or_else(|| anyhow::anyhow!("input has no audio stream"))?;
    let stream_index = stream.index();
    let context = codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = context.decoder().audio()?;
    let input_layout = audio_channel_layout(&decoder);
    let mut resampler = resampling::Context::get(
        decoder.format(),
        input_layout,
        decoder.rate(),
        Sample::F32(format::sample::Type::Planar),
        SILENCE_CHANNEL_LAYOUT,
        SILENCE_SAMPLE_RATE,
    )?;

    if seek_seconds.is_finite() && seek_seconds > 0.0 {
        let seek_ts = (seek_seconds * 1_000_000.0).round().max(0.0) as i64;
        ictx.seek(seek_ts, ..seek_ts)
            .or_else(|_| ictx.seek(seek_ts, ..))?;
    }

    let max_seconds = duration_seconds.min(min_silence_seconds).max(0.0);
    let max_samples = (max_seconds * f64::from(SILENCE_SAMPLE_RATE)).ceil() as usize;
    let threshold = 10.0_f32.powf(threshold_db / 20.0).abs();
    let mut analyzed_samples = 0_usize;
    let mut has_loud_sample = false;

    for (stream, packet) in ictx.packets() {
        if cancelled() {
            return Err(ffmpeg_next::Error::Exit.into());
        }

        if stream.index() != stream_index {
            continue;
        }

        decoder.send_packet(&packet)?;
        receive_silence_frames(
            &mut decoder,
            &mut resampler,
            input_layout,
            threshold,
            max_samples,
            &mut analyzed_samples,
            &mut has_loud_sample,
        )?;

        if has_loud_sample || analyzed_samples >= max_samples {
            break;
        }
    }

    if cancelled() {
        return Err(ffmpeg_next::Error::Exit.into());
    }

    decoder.send_eof()?;
    receive_silence_frames(
        &mut decoder,
        &mut resampler,
        input_layout,
        threshold,
        max_samples,
        &mut analyzed_samples,
        &mut has_loud_sample,
    )?;

    let analyzed_seconds = analyzed_samples as f64 / f64::from(SILENCE_SAMPLE_RATE);

    Ok(SilenceDetection {
        silent: !has_loud_sample && analyzed_seconds >= min_silence_seconds,
        analyzed_seconds,
    })
}

fn receive_silence_frames(
    decoder: &mut codec::decoder::Audio,
    resampler: &mut resampling::Context,
    input_layout: ChannelLayout,
    threshold: f32,
    max_samples: usize,
    analyzed_samples: &mut usize,
    has_loud_sample: &mut bool,
) -> Result<()> {
    let mut raw = frame::Audio::empty();

    while decoder.receive_frame(&mut raw).is_ok() {
        if raw.channel_layout().is_empty() {
            raw.set_channel_layout(input_layout);
        }

        let mut converted = frame::Audio::empty();
        resampler.run(&raw, &mut converted)?;
        let remaining = max_samples.saturating_sub(*analyzed_samples);

        if remaining == 0 {
            return Ok(());
        }

        let samples = converted.samples().min(remaining);

        for channel in 0..converted.planes() {
            if converted.plane::<f32>(channel)[..samples]
                .iter()
                .any(|sample| sample.abs() > threshold)
            {
                *has_loud_sample = true;
                break;
            }
        }
        *analyzed_samples += samples;

        if *has_loud_sample || *analyzed_samples >= max_samples {
            return Ok(());
        }
    }

    Ok(())
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

    // SAFETY: parameters borrow the live input, and no decoder is needed to
    // inspect the format already discovered by avformat_find_stream_info.
    unsafe {
        let parameters = &*parameters.as_ptr();
        result.sample_rate = Some(i64::from(parameters.sample_rate));
        result.channels = Some(i64::from(parameters.ch_layout.nb_channels));
    }

    result
}

fn audio_channel_layout(decoder: &codec::decoder::Audio) -> ChannelLayout {
    let channel_layout = decoder.channel_layout();

    if channel_layout.is_empty() {
        ChannelLayout::default(i32::from(decoder.channels()).max(1))
    } else {
        channel_layout
    }
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

    // SAFETY: the stream and its parameters outlive these read-only accesses.
    unsafe {
        let parameters = &*parameters.as_ptr();
        let width = parameters.width;
        let height = parameters.height;
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

    let duration = hours * 3_600.0 + minutes * 60.0 + seconds;
    (duration > 0.0).then_some(duration)
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::detect_audio_silence;

    fn media_mix_asset(name: &str) -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_mix")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn stream_metadata_does_not_require_an_available_decoder() {
        use ffmpeg_next::{ffi, format};
        // SAFETY: the RAII input owns the allocated context and its stream.
        // No decoder is available for AV_CODEC_ID_NONE; parameter inspection
        // must still return the container's audio/video metadata.
        unsafe {
            let context = ffi::avformat_alloc_context();
            assert!(!context.is_null());
            let input = format::context::Input::wrap(context);
            let stream = ffi::avformat_new_stream(context, std::ptr::null());
            assert!(!stream.is_null());
            (*(*stream).codecpar).width = 640;
            (*(*stream).codecpar).height = 360;
            (*(*stream).codecpar).sample_rate = 48_000;
            (*(*stream).codecpar).ch_layout.nb_channels = 2;
            let stream = input.stream(0).unwrap();
            let video = super::probe_video_stream(&stream);
            assert_eq!((video.width, video.height), (Some(640), Some(360)));
            let audio = super::probe_audio_stream(&stream);
            assert_eq!((audio.sample_rate, audio.channels), (Some(48_000), Some(2)));
        }
    }

    #[test]
    fn cancelled_silence_analysis_does_not_open_the_input() {
        let error = super::detect_audio_silence_with_cancel(
            "/file-that-does-not-exist",
            0.0,
            30.0,
            -30.0,
            30.0,
            || true,
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<ffmpeg_next::Error>(),
            Some(&ffmpeg_next::Error::Exit)
        );
    }

    #[test]
    fn silence_analysis_observes_cancellation_after_open() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        let error = super::detect_audio_silence_with_cancel(
            &media_mix_asset("with_audio.mp4"),
            0.0,
            30.0,
            -30.0,
            30.0,
            move || token.swap(true, Ordering::SeqCst),
        )
        .unwrap_err();
        assert!(cancelled.load(Ordering::SeqCst));
        assert_eq!(
            error.downcast_ref::<ffmpeg_next::Error>(),
            Some(&ffmpeg_next::Error::Exit)
        );
    }

    #[test]
    fn silence_detection_rejects_media_without_audio_stream() {
        ffmpeg_next::init().ok();
        let error = detect_audio_silence(&media_mix_asset("no_audio.mp4"), 0.0, 15.0, -30.0, 15.0)
            .expect_err("no_audio.mp4 must not be analyzed as silent audio");

        assert!(
            error.to_string().contains("input has no audio stream"),
            "{error:#}"
        );
    }
}
