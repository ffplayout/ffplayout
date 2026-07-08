use std::{collections::VecDeque, fs, path::Path};

use anyhow::{Context, Result, anyhow};
use ffmpeg::{
    Packet, codec, format, frame,
    software::scaling,
    util::{
        channel_layout::ChannelLayout, format::pixel::Pixel, format::sample::Sample,
        rational::Rational,
    },
};
use ffmpeg_next as ffmpeg;

use super::{hls, vtt};
use crate::{
    analysis::audio_level::AudioLevelMeter,
    audio_mixer::AudioEffectChain,
    clock::PlayoutClock,
    utils::{
        config::{HlsSubtitle, HlsVariant, OutputConfig, RateControl},
        helper::{is_network_url, network_io_options},
    },
};

pub(super) struct EncodedOutput {
    octx: format::context::Output,
    video_streams: Vec<VideoOutputStream>,
    audio_streams: Vec<AudioOutputStream>,
    subtitle_streams: Vec<SubtitleOutputStream>,
    vtt_subtitles: bool,
    audio_effects: AudioEffectChain,
    audio_level_meter: AudioLevelMeter,
    audio_buffer: [VecDeque<f32>; 2],
    audio_buffer_pts: Option<i64>,
    audio_sample_rate: u32,
    clock: PlayoutClock,
}

#[derive(Clone)]
pub(super) enum EncodedFormat {
    Auto,
    Hls {
        variants: Vec<HlsVariant>,
        subtitle: Option<HlsSubtitle>,
        segment_seconds: u32,
        list_size: u32,
    },
}

struct VideoOutputStream {
    stream_index: usize,
    encoder: codec::encoder::video::Encoder,
    scaler: Option<scaling::Context>,
    scaled_frame: Option<frame::Video>,
    width: u32,
    height: u32,
}

struct AudioOutputStream {
    stream_index: usize,
    encoder: codec::encoder::audio::Encoder,
}

struct SubtitleOutputStream {
    stream_index: usize,
}

impl EncodedOutput {
    pub(super) fn open(
        path: &str,
        cfg: &OutputConfig,
        output_format: EncodedFormat,
    ) -> Result<Self> {
        let hls_variants = match &output_format {
            EncodedFormat::Auto => &[][..],
            EncodedFormat::Hls { variants, .. } => variants.as_slice(),
        };
        let hls_subtitle = match &output_format {
            EncodedFormat::Hls { subtitle, .. } => subtitle.as_ref(),
            EncodedFormat::Auto => None,
        };
        let vtt_subtitles = hls_subtitle.is_some();
        hls::validate_variants(hls_variants)?;
        if let Some(subtitle) = hls_subtitle {
            subtitle.validate().map_err(anyhow::Error::msg)?;
        }

        // ffmpeg's HLS muxer only emits a master playlist (with the
        // `EXT-X-MEDIA:TYPE=SUBTITLES` entry HLS players need to discover the
        // VTT track) when `var_stream_map` is used. Some ffmpeg versions also
        // require a `%v` playlist template whenever `var_stream_map` is set,
        // even if there is only one implicit variant. For VTT-only output we
        // therefore synthesize a single default variant named after the
        // requested playlist stem, so `%v.m3u8` still resolves to the literal
        // target such as `index.m3u8`. It doesn't affect real encoder
        // settings: `open_video_stream`/`open_audio_stream` still fall back to
        // `cfg` because they receive `None` for their `variant` argument below.
        let default_variant_name = Path::new(path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("stream")
            .to_string();
        let default_variant = [HlsVariant {
            name: default_variant_name,
            width: cfg.width,
            height: cfg.height,
            video_bitrate: 0,
            audio_bitrate: 0,
        }];
        let uses_var_stream_map = !hls_variants.is_empty() || vtt_subtitles;
        let variants_for_naming: &[HlsVariant] = if hls_variants.is_empty() && vtt_subtitles {
            &default_variant
        } else {
            hls_variants
        };

        let hls_playlist_path = hls::playlist_path(path, hls_variants)?;
        let hls_output_path = if uses_var_stream_map {
            hls::playlist_path(path, variants_for_naming)?
        } else {
            hls_playlist_path.clone()
        };

        if matches!(output_format, EncodedFormat::Hls { .. })
            && let Some(parent) = Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create HLS directory {}", parent.display()))?;
        }
        if matches!(output_format, EncodedFormat::Hls { .. }) && !uses_var_stream_map {
            hls::remove_master_playlist(path)?;
        }
        let hls_start_number = if matches!(output_format, EncodedFormat::Hls { .. }) {
            let resume_playlists =
                hls_resume_playlist_paths(path, &hls_playlist_path, hls_variants)?;
            let master_playlist = uses_var_stream_map.then(|| {
                hls::master_playlist_path(path)
                    .to_string_lossy()
                    .into_owned()
            });
            hls::prepare_resume_start_number(&resume_playlists, master_playlist.as_deref())?
        } else {
            None
        };
        // Network outputs get a write timeout so a stalled TCP connection
        // surfaces as an error instead of blocking the playout worker forever.
        let mut octx = match output_format {
            EncodedFormat::Hls { .. } => format::output_as(&hls_output_path, "hls")?,
            EncodedFormat::Auto if path.starts_with("rtmp://") || path.starts_with("rtmps://") => {
                format::output_as_with(path, "flv", network_io_options())?
            }
            EncodedFormat::Auto if is_network_url(path) => {
                format::output_with(path, network_io_options())?
            }
            EncodedFormat::Auto => format::output(path)?,
        };
        // `format::output_as` preopens the `%v.m3u8` template path before the
        // HLS muxer substitutes the concrete variant name. Close and remove
        // that placeholder so only the real media playlists remain.
        if uses_var_stream_map {
            hls::close_preopened_output(&mut octx, &hls_output_path)?;
        }

        let global_header = octx
            .format()
            .flags()
            .contains(format::flag::Flags::GLOBAL_HEADER);

        let mut video_streams = Vec::new();
        let mut audio_streams = Vec::new();
        let mut subtitle_streams = Vec::new();
        let stream_count = hls_variants.len().max(1);

        for index in 0..stream_count {
            let variant = hls_variants.get(index);
            video_streams.push(open_video_stream(
                &mut octx,
                cfg,
                output_format.clone(),
                global_header,
                variant,
            )?);
            audio_streams.push(open_audio_stream(&mut octx, cfg, global_header, variant)?);
        }
        if vtt_subtitles {
            subtitle_streams.push(open_subtitle_stream(&mut octx)?);
        }

        match output_format {
            EncodedFormat::Auto => octx.write_header()?,
            EncodedFormat::Hls {
                segment_seconds,
                list_size,
                ..
            } => {
                let hls_flags = if hls_start_number.is_some() {
                    "delete_segments+omit_endlist+temp_file+discont_start"
                } else {
                    "delete_segments+omit_endlist+temp_file"
                };
                let mut options = ffmpeg::Dictionary::new();
                options.set("hls_time", &segment_seconds.to_string());
                options.set("hls_list_size", &list_size.to_string());
                options.set("hls_flags", hls_flags);
                let segment_filename = if uses_var_stream_map {
                    hls::segment_pattern(path)
                } else {
                    hls::standalone_segment_pattern(path)
                };
                options.set("hls_segment_filename", &segment_filename);
                if let Some(start_number) = hls_start_number {
                    options.set("start_number", &start_number.to_string());
                }
                if uses_var_stream_map {
                    options.set("master_pl_name", "master.m3u8");
                    options.set(
                        "var_stream_map",
                        &hls::var_stream_map(variants_for_naming, hls_subtitle),
                    );
                }
                reject_unused_options(octx.write_header_with(options)?)?;
            }
        }

        Ok(Self {
            octx,
            video_streams,
            audio_streams,
            subtitle_streams,
            vtt_subtitles,
            audio_effects: AudioEffectChain::new(cfg.audio_effects.clone(), cfg.sample_rate),
            audio_level_meter: AudioLevelMeter::new(
                cfg.sample_rate,
                cfg.audio_level_callback.clone(),
            ),
            audio_buffer: [VecDeque::new(), VecDeque::new()],
            audio_buffer_pts: None,
            audio_sample_rate: cfg.sample_rate,
            clock: PlayoutClock::new(),
        })
    }

    pub(super) fn audio_frame_size(&self) -> usize {
        self.audio_streams
            .first()
            .map(|stream| stream.encoder.frame_size() as usize)
            .unwrap_or(0)
    }

    pub(super) fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        for index in 0..self.video_streams.len() {
            let stream = &mut self.video_streams[index];
            if let Some(scaler) = &mut stream.scaler {
                let scaled_frame = stream.scaled_frame.get_or_insert_with(|| {
                    frame::Video::new(Pixel::YUV420P, stream.width, stream.height)
                });
                scaled_frame.set_pts(frame.pts());
                scaler.run(frame, scaled_frame)?;
                stream.encoder.send_frame(scaled_frame)?;
            } else {
                stream.encoder.send_frame(frame)?;
            }
            self.write_video_packets(index)?;
        }
        Ok(())
    }

    pub(super) fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        if frame.samples() == 0 {
            return Ok(());
        }

        let mut frame = frame.clone();
        self.audio_effects.process(&mut frame);
        self.audio_level_meter.process_frame(&frame);
        self.align_audio_buffer_to_frame_pts(frame.pts())?;
        if self.audio_buffer[0].is_empty() {
            self.audio_buffer_pts = frame.pts();
        }
        for channel in 0..self.audio_buffer.len() {
            self.audio_buffer[channel].extend(
                frame
                    .plane::<f32>(channel)
                    .iter()
                    .map(|sample| if sample.is_finite() { *sample } else { 0.0 }),
            );
        }

        self.write_complete_audio_frames()
    }

    fn align_audio_buffer_to_frame_pts(&mut self, frame_pts: Option<i64>) -> Result<()> {
        let Some(frame_pts) = frame_pts else {
            return Ok(());
        };
        let Some(buffer_pts) = self.audio_buffer_pts else {
            return Ok(());
        };
        if self.audio_buffer[0].is_empty() {
            return Ok(());
        }

        let expected_pts = buffer_pts + self.audio_buffer[0].len() as i64;
        if frame_pts != expected_pts {
            self.pad_audio_buffer()?;
            if self.audio_buffer[0].is_empty() {
                self.audio_buffer_pts = Some(frame_pts);
            }
        }

        Ok(())
    }

    pub(super) fn write_vtt_subtitles(
        &mut self,
        media_path: &str,
        output_start_ms: i64,
        source_start_ms: i64,
    ) -> Result<()> {
        if !self.vtt_subtitles || self.subtitle_streams.is_empty() {
            return Ok(());
        }

        let vtt_path = vtt::sidecar_path(media_path);
        if !vtt_path.exists() {
            return Ok(());
        }

        let cues = vtt::parse_file(&vtt_path)?;
        for cue in cues {
            if cue.end_ms <= source_start_ms {
                continue;
            }

            let mut packet = Packet::copy(cue.text.as_bytes());
            let cue_start_ms = cue.start_ms.saturating_sub(source_start_ms);
            let cue_end_ms = cue.end_ms - source_start_ms;
            let pts = output_start_ms + cue_start_ms;
            packet.set_pts(Some(pts));
            packet.set_dts(Some(pts));
            packet.set_duration(cue_end_ms - cue_start_ms);
            self.write_subtitle_packet(&mut packet)?;
        }

        Ok(())
    }

    fn write_complete_audio_frames(&mut self) -> Result<()> {
        let frame_size = self.audio_frame_size();
        if frame_size == 0 {
            return Err(anyhow!("audio encoder reported a frame size of zero"));
        }

        while self
            .audio_buffer
            .iter()
            .all(|channel| channel.len() >= frame_size)
        {
            let mut frame = frame::Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Planar),
                frame_size,
                ChannelLayout::STEREO,
            );
            frame.set_rate(self.audio_sample_rate);
            frame.set_pts(self.audio_buffer_pts);

            for channel in 0..self.audio_buffer.len() {
                let plane = frame.plane_mut::<f32>(channel);
                let buffer = &mut self.audio_buffer[channel];
                let (front, back) = buffer.as_slices();
                let from_front = front.len().min(frame_size);
                plane[..from_front].copy_from_slice(&front[..from_front]);
                plane[from_front..frame_size].copy_from_slice(&back[..frame_size - from_front]);
                buffer.drain(..frame_size);
            }

            self.audio_buffer_pts = self.audio_buffer_pts.map(|pts| pts + frame_size as i64);
            self.send_audio_frame(&frame)?;
        }

        Ok(())
    }

    fn send_audio_frame(&mut self, frame: &frame::Audio) -> Result<()> {
        for index in 0..self.audio_streams.len() {
            self.audio_streams[index].encoder.send_frame(frame)?;
            self.write_audio_packets(index)?;
        }
        Ok(())
    }

    fn pad_audio_buffer(&mut self) -> Result<()> {
        if self.audio_buffer[0].is_empty() {
            return Ok(());
        }

        let frame_size = self.audio_frame_size();
        for channel in &mut self.audio_buffer {
            channel.resize(frame_size, 0.0);
        }
        self.write_complete_audio_frames()
    }

    fn write_packet(
        &mut self,
        packet: &mut ffmpeg::Packet,
        stream_index: usize,
        encoder_time_base: Rational,
    ) -> Result<()> {
        let stream_time_base = self
            .octx
            .stream(stream_index)
            .context("output stream is missing")?
            .time_base();

        packet.set_stream(stream_index);
        packet.rescale_ts(encoder_time_base, stream_time_base);
        self.clock
            .wait_until(packet.dts().or_else(|| packet.pts()), stream_time_base);
        packet.write_interleaved(&mut self.octx)?;
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<()> {
        for index in 0..self.video_streams.len() {
            self.video_streams[index].encoder.send_eof()?;
            self.write_video_packets(index)?;
        }

        self.pad_audio_buffer()?;
        for index in 0..self.audio_streams.len() {
            self.audio_streams[index].encoder.send_eof()?;
            self.write_audio_packets(index)?;
        }

        self.octx.write_trailer()?;
        Ok(())
    }

    fn write_video_packets(&mut self, index: usize) -> Result<()> {
        let mut packet = ffmpeg::Packet::empty();
        while self.video_streams[index]
            .encoder
            .receive_packet(&mut packet)
            .is_ok()
        {
            if packet.duration() == 0 {
                packet.set_duration(1);
            }
            let stream_index = self.video_streams[index].stream_index;
            let time_base = self.video_streams[index].encoder.time_base();
            self.write_packet(&mut packet, stream_index, time_base)?;
        }
        Ok(())
    }

    fn write_audio_packets(&mut self, index: usize) -> Result<()> {
        let mut packet = ffmpeg::Packet::empty();
        while self.audio_streams[index]
            .encoder
            .receive_packet(&mut packet)
            .is_ok()
        {
            let stream_index = self.audio_streams[index].stream_index;
            let time_base = self.audio_streams[index].encoder.time_base();
            self.write_packet(&mut packet, stream_index, time_base)?;
        }
        Ok(())
    }

    fn write_subtitle_packet(&mut self, packet: &mut Packet) -> Result<()> {
        let stream_index = self
            .subtitle_streams
            .first()
            .context("subtitle output stream is missing")?
            .stream_index;
        let stream_time_base = self
            .octx
            .stream(stream_index)
            .context("subtitle output stream is missing")?
            .time_base();

        packet.set_stream(stream_index);
        packet.rescale_ts(Rational(1, 1_000), stream_time_base);
        packet.write_interleaved(&mut self.octx)?;
        Ok(())
    }
}

fn reject_unused_options(options: ffmpeg::Dictionary<'_>) -> Result<()> {
    let unused = options
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    if unused.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "unused FFmpeg output option(s): {}",
            unused.join(", ")
        ))
    }
}

fn hls_resume_playlist_paths(
    path: &str,
    hls_playlist_path: &str,
    variants: &[HlsVariant],
) -> Result<Vec<String>> {
    if variants.is_empty() {
        Ok(vec![hls_playlist_path.to_string()])
    } else {
        variants
            .iter()
            .map(|variant| hls::resolved_variant_playlist_path(path, &variant.name))
            .collect()
    }
}

fn open_video_stream(
    octx: &mut format::context::Output,
    cfg: &OutputConfig,
    output_format: EncodedFormat,
    global_header: bool,
    variant: Option<&HlsVariant>,
) -> Result<VideoOutputStream> {
    let video_codec = codec::encoder::find(codec::Id::H264).context("H.264 encoder not found")?;
    let mut video_stream = octx.add_stream(video_codec)?;
    let mut video_ctx = codec::context::Context::new_with_codec(video_codec)
        .encoder()
        .video()?;
    let width = variant.map_or(cfg.width, |variant| variant.width);
    let height = variant.map_or(cfg.height, |variant| variant.height);
    video_ctx.set_width(width);
    video_ctx.set_height(height);
    video_ctx.set_format(Pixel::YUV420P);
    video_ctx.set_time_base(cfg.video_time_base);
    video_ctx.set_frame_rate(Some(Rational(cfg.fps as i32, 1)));
    let maxrate = variant.map_or(cfg.video_maxrate, |variant| variant.video_bitrate);
    if cfg.rate_control == RateControl::Cbr {
        video_ctx.set_bit_rate(maxrate as usize);
    }
    if matches!(output_format, EncodedFormat::Hls { .. }) {
        video_ctx.set_gop(cfg.fps.saturating_mul(2));
    }
    let mut video_flags = codec::flag::Flags::empty();
    if global_header {
        video_flags |= codec::flag::Flags::GLOBAL_HEADER;
    }
    if matches!(output_format, EncodedFormat::Hls { .. }) {
        video_flags |= codec::flag::Flags::CLOSED_GOP;
    }
    if !video_flags.is_empty() {
        video_ctx.set_flags(video_flags);
    }

    let mut options = ffmpeg::Dictionary::new();
    options.set("preset", &cfg.video_preset);
    options.set("tune", "zerolatency");
    options.set("maxrate", &maxrate.to_string());
    options.set("bufsize", &maxrate.saturating_mul(2).to_string());
    match cfg.rate_control {
        RateControl::Crf => options.set("crf", &cfg.video_quality.to_string()),
        RateControl::Cbr => options.set("minrate", &maxrate.to_string()),
    }

    let mut video_encoder = match output_format {
        EncodedFormat::Auto => video_ctx.open_as_with(video_codec, options)?,
        EncodedFormat::Hls { .. } => {
            options.set("x264-params", "open-gop=0:repeat-headers=1");
            video_ctx.open_as_with(video_codec, options)?
        }
    };
    // CRF encoders clear AVCodecContext::bit_rate while opening. Restore the
    // configured maximum as metadata so the HLS muxer can calculate BANDWIDTH
    // for master.m3u8; this does not change the already-open encoder mode.
    video_encoder.set_bit_rate(maxrate as usize);
    video_stream.set_parameters(&video_encoder);
    video_stream.set_time_base(cfg.video_time_base);
    let stream_index = video_stream.index();
    let scaler = if width == cfg.width && height == cfg.height {
        None
    } else {
        Some(scaling::Context::get(
            Pixel::YUV420P,
            cfg.width,
            cfg.height,
            Pixel::YUV420P,
            width,
            height,
            scaling::flag::Flags::BILINEAR,
        )?)
    };

    Ok(VideoOutputStream {
        stream_index,
        encoder: video_encoder,
        scaler,
        scaled_frame: None,
        width,
        height,
    })
}

fn open_audio_stream(
    octx: &mut format::context::Output,
    cfg: &OutputConfig,
    global_header: bool,
    variant: Option<&HlsVariant>,
) -> Result<AudioOutputStream> {
    let audio_codec = codec::encoder::find(codec::Id::AAC).context("AAC encoder not found")?;
    let mut audio_stream = octx.add_stream(audio_codec)?;
    let mut audio_ctx = codec::context::Context::new_with_codec(audio_codec)
        .encoder()
        .audio()?;
    audio_ctx.set_rate(cfg.sample_rate as i32);
    audio_ctx.set_channel_layout(ChannelLayout::STEREO);
    audio_ctx.set_format(Sample::F32(ffmpeg::format::sample::Type::Planar));
    audio_ctx.set_time_base(cfg.audio_time_base);
    audio_ctx.set_bit_rate(variant.map_or(cfg.audio_bitrate as usize, |variant| {
        variant.audio_bitrate as usize
    }));
    if global_header {
        audio_ctx.set_flags(codec::flag::Flags::GLOBAL_HEADER);
    }
    let audio_encoder = audio_ctx.open_as(audio_codec)?;
    audio_stream.set_parameters(&audio_encoder);
    audio_stream.set_time_base(cfg.audio_time_base);
    Ok(AudioOutputStream {
        stream_index: audio_stream.index(),
        encoder: audio_encoder,
    })
}

fn open_subtitle_stream(octx: &mut format::context::Output) -> Result<SubtitleOutputStream> {
    let mut stream = octx.add_stream(codec::Id::WEBVTT)?;
    stream.set_time_base(Rational(1, 1_000));
    // `add_stream` only sets up an encoder-backed stream; WebVTT subtitles here
    // are muxed as pre-formatted text packets without an actual encoder, so the
    // safe API has no way to mark the stream's codec parameters as a subtitle
    // stream. Setting `codec_type`/`codec_id` on the raw `AVCodecParameters` is
    // the only way to make the muxer (and downstream HLS players) recognize it.
    unsafe {
        let codecpar = (*stream.as_mut_ptr()).codecpar;
        (*codecpar).codec_type = ffmpeg::ffi::AVMediaType::AVMEDIA_TYPE_SUBTITLE;
        (*codecpar).codec_id = ffmpeg::ffi::AVCodecID::AV_CODEC_ID_WEBVTT;
    }
    Ok(SubtitleOutputStream {
        stream_index: stream.index(),
    })
}

#[cfg(test)]
mod open_tests {
    use super::*;
    use crate::utils::{
        config::{HlsSubtitle, OutputConfig},
        ffmpeg_capabilities::ffmpeg_capabilities,
    };
    use std::fs;

    #[test]
    fn vtt_only_master_playlist_uses_literal_playlist_name() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("hls_vtt_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.m3u8");
        let cfg = OutputConfig::new(320, 240, 25, 44100);
        let output = EncodedOutput::open(
            path.to_str().unwrap(),
            &cfg,
            EncodedFormat::Hls {
                variants: vec![],
                subtitle: Some(HlsSubtitle {
                    name: "Subtitles".to_string(),
                    language: "und".to_string(),
                    default: false,
                }),
                segment_seconds: 6,
                list_size: 60,
            },
        );
        let output = output.unwrap();
        assert!(
            output.finish().is_ok(),
            "expected finish() to flush the trailer"
        );
        assert!(path.exists(), "expected literal index.m3u8 to exist");
        let master = fs::read_to_string(dir.join("master.m3u8")).unwrap();
        if ffmpeg_capabilities().features.hls_subtitle_name {
            assert!(master.contains("NAME=\"Subtitles\""), "{master}");
        } else {
            assert!(master.contains("TYPE=SUBTITLES"), "{master}");
        }
        assert!(master.contains("LANGUAGE=\"und\""), "{master}");
        assert!(master.contains("DEFAULT=NO"), "{master}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn standalone_hls_output_does_not_create_master_playlist() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("hls_standalone_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stream.m3u8");
        fs::write(dir.join("master.m3u8"), "stale").unwrap();
        let cfg = OutputConfig::new(320, 240, 25, 44100);
        let output = EncodedOutput::open(
            path.to_str().unwrap(),
            &cfg,
            EncodedFormat::Hls {
                variants: vec![],
                subtitle: None,
                segment_seconds: 6,
                list_size: 60,
            },
        )
        .unwrap();

        output.finish().unwrap();
        assert!(path.exists(), "expected stream.m3u8 to exist");
        assert!(
            !dir.join("master.m3u8").exists(),
            "standalone HLS output must not create master.m3u8"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cbr_encoder_options_are_accepted() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("hls_cbr_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.m3u8");
        let cfg = OutputConfig::new(320, 240, 25, 44100).with_encoding(
            "faster".to_string(),
            RateControl::Cbr,
            23,
            1_300_000,
            128_000,
        );
        let output = EncodedOutput::open(
            path.to_str().unwrap(),
            &cfg,
            EncodedFormat::Hls {
                variants: vec![],
                subtitle: None,
                segment_seconds: 6,
                list_size: 60,
            },
        );

        assert!(output.is_ok(), "expected CBR encoder options to be valid");
        drop(output);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn standalone_hls_appends_after_existing_playlist_sequence() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("hls_append_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stream.m3u8");

        let cfg = OutputConfig::new(320, 240, 25, 44100);

        for brightness in [16, 160] {
            let mut output = EncodedOutput::open(
                path.to_str().unwrap(),
                &cfg,
                EncodedFormat::Hls {
                    variants: vec![],
                    subtitle: None,
                    segment_seconds: 1,
                    list_size: 60,
                },
            )
            .unwrap();

            for index in 0..60 {
                let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
                video.set_pts(Some(index));
                video.data_mut(0).fill(brightness);
                output.encode_video(&video).unwrap();
                let mut audio = frame::Audio::new(
                    Sample::F32(ffmpeg::format::sample::Type::Planar),
                    output.audio_frame_size(),
                    ChannelLayout::STEREO,
                );
                audio.set_rate(44100);
                audio.set_pts(Some(index * output.audio_frame_size() as i64));
                for channel in 0..2 {
                    audio.plane_mut::<f32>(channel).fill(0.0);
                }
                output.encode_audio(&audio).unwrap();
            }
            output.finish().unwrap();

            if brightness == 16 {
                let first_segment = fs::read(dir.join("stream_0.ts")).unwrap();
                assert!(!first_segment.is_empty());
                fs::write(dir.join("stream_0.snapshot"), first_segment).unwrap();
            }
        }

        let playlist = fs::read_to_string(&path).unwrap();
        assert!(playlist.contains("stream_2.ts"), "{playlist}");
        assert_eq!(
            fs::read(dir.join("stream_0.ts")).unwrap(),
            fs::read(dir.join("stream_0.snapshot")).unwrap()
        );
        assert!(dir.join("stream_2.ts").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn master_playlist_contains_base_output_and_additional_variant() {
        ffmpeg::init().ok();
        let dir =
            std::env::temp_dir().join(format!("hls_multiple_streams_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stream.m3u8");
        let cfg = OutputConfig::new(320, 240, 25, 44100);
        let variants = vec![
            HlsVariant {
                name: "stream".to_string(),
                width: 320,
                height: 240,
                video_bitrate: 1_300_000,
                audio_bitrate: 128_000,
            },
            HlsVariant {
                name: "low".to_string(),
                width: 160,
                height: 120,
                video_bitrate: 600_000,
                audio_bitrate: 96_000,
            },
        ];
        let mut output = EncodedOutput::open(
            path.to_str().unwrap(),
            &cfg,
            EncodedFormat::Hls {
                variants,
                subtitle: None,
                segment_seconds: 6,
                list_size: 60,
            },
        )
        .unwrap();

        for stream in output.octx.streams() {
            let parameters = stream.parameters();
            let bit_rate = unsafe { (*parameters.as_ptr()).bit_rate };
            assert!(bit_rate > 0, "stream {} has no bitrate", stream.index());
        }
        for index in 0..16 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            video.set_pts(Some(index));
            output.encode_video(&video).unwrap();
            let mut audio = frame::Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Planar),
                output.audio_frame_size(),
                ChannelLayout::STEREO,
            );
            audio.set_rate(44100);
            audio.set_pts(Some(index * output.audio_frame_size() as i64));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            output.encode_audio(&audio).unwrap();
        }
        output.finish().unwrap();
        let master = fs::read_to_string(dir.join("master.m3u8")).unwrap();
        assert!(master.contains("stream.m3u8"), "{master}");
        assert!(master.contains("low.m3u8"), "{master}");
        fs::remove_dir_all(&dir).ok();
    }
}
