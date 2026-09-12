use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::CString,
    fs,
    path::Path,
    ptr,
};

use anyhow::{Context, Result, anyhow};
use ffmpeg::{
    Packet, codec, format, frame,
    software::{resampling, scaling},
    util::{
        channel_layout::ChannelLayout, format::pixel::Pixel, format::sample::Sample,
        rational::Rational,
    },
};
use ffmpeg_next as ffmpeg;

use super::{
    hls,
    recording::{self, RecordingMonitor, RecordingMuxer},
    vtt,
};
use crate::{
    HlsHealth,
    analysis::{audio_level::AudioLevelMeter, loudness::LoudnessMeter},
    audio_mixer::AudioEffectChain,
    benchmark::{self, Stage},
    clock::PlayoutClock,
    utils::{
        config::{
            HlsSubtitle, HlsVariant, OutputConfig, audio_encoder_context,
            engine_audio_sample_format, video_codec_uses_bitrate,
        },
        ffmpeg_capabilities::validate_muxer_options,
        helper::{is_network_url, network_io_options},
    },
};

pub(super) struct EncodedOutput {
    octx: format::context::Output,
    video_streams: Vec<VideoOutputStream>,
    audio_streams: Vec<AudioOutputStream>,
    subtitle_streams: Vec<SubtitleOutputStream>,
    vtt_subtitles: bool,
    pending_vtt_cues: VecDeque<vtt::VttCue>,
    audio_effects: AudioEffectChain,
    audio_level_meter: AudioLevelMeter,
    loudness_meter: LoudnessMeter,
    audio_buffer: [VecDeque<f32>; 2],
    audio_buffer_pts: Option<i64>,
    audio_sample_rate: u32,
    clock: PlayoutClock,
    pace_output: bool,
    hls_health: Option<HlsHealth>,
    recording: Option<RecordingMuxer>,
    recording_video_stream_index: usize,
    transcoded_recording: Option<Box<Self>>,
    recording_monitor: Option<RecordingMonitor>,
    channel_id: Option<i32>,
}

#[derive(Clone)]
pub(super) enum EncodedFormat {
    Auto,
    Stream {
        muxer: String,
    },
    Hls {
        variants: Vec<HlsVariant>,
        subtitle: Option<HlsSubtitle>,
        segment_seconds: u32,
        list_size: u32,
    },
    Recording {
        segment_seconds: u32,
        input_width: u32,
        input_height: u32,
    },
}

struct VideoOutputStream {
    stream_index: usize,
    encoder: codec::encoder::video::Encoder,
    scaler: Option<scaling::Context>,
    scaled_frame: Option<frame::Video>,
    vaapi_upload: Option<VaapiUpload>,
}

/// Owns the VAAPI frame pool used to upload the CPU-composited NV12 frame
/// immediately before passing it to a VAAPI encoder.
struct VaapiUpload {
    frames_ctx: *mut ffmpeg::ffi::AVBufferRef,
    frame: frame::Video,
}

unsafe impl Send for VaapiUpload {}

impl VaapiUpload {
    fn new(width: u32, height: u32) -> Result<Self> {
        let device = CString::new("/dev/dri/renderD128").expect("static VAAPI device path");
        let mut device_ctx = ptr::null_mut();
        let result = unsafe {
            ffmpeg::ffi::av_hwdevice_ctx_create(
                &mut device_ctx,
                ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                device.as_ptr(),
                ptr::null_mut(),
                0,
            )
        };
        if result < 0 {
            return Err(ffmpeg::Error::from(result))
                .context("failed to create VAAPI device /dev/dri/renderD128");
        }
        if device_ctx.is_null() {
            return Err(anyhow!("VAAPI device creation returned no device context"));
        }

        let frames_ctx = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(device_ctx) };
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut device_ctx) };
        if frames_ctx.is_null() {
            return Err(anyhow!("failed to allocate VAAPI frame context"));
        }

        let frames = unsafe { (*frames_ctx).data.cast::<ffmpeg::ffi::AVHWFramesContext>() };
        if frames.is_null() {
            unsafe { ffmpeg::ffi::av_buffer_unref(&mut (frames_ctx as *mut _)) };
            return Err(anyhow!("VAAPI frame context has no frame pool data"));
        }
        unsafe {
            (*frames).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
            (*frames).sw_format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_NV12;
            (*frames).width = width as i32;
            (*frames).height = height as i32;
        }
        let result = unsafe { ffmpeg::ffi::av_hwframe_ctx_init(frames_ctx) };
        if result < 0 {
            unsafe { ffmpeg::ffi::av_buffer_unref(&mut (frames_ctx as *mut _)) };
            return Err(ffmpeg::Error::from(result))
                .context("failed to initialize VAAPI frame context");
        }

        Ok(Self {
            frames_ctx,
            frame: frame::Video::empty(),
        })
    }

    fn attach_to_encoder(&self, video_ctx: &mut codec::encoder::video::Video) -> Result<()> {
        let frames_ctx = unsafe { ffmpeg::ffi::av_buffer_ref(self.frames_ctx) };
        if frames_ctx.is_null() {
            return Err(anyhow!("failed to retain VAAPI frame context for encoder"));
        }
        unsafe { (*video_ctx.as_mut_ptr()).hw_frames_ctx = frames_ctx };
        Ok(())
    }

    fn upload(&mut self, source: &frame::Video) -> Result<&frame::Video> {
        unsafe { ffmpeg::ffi::av_frame_unref(self.frame.as_mut_ptr()) };
        let result = unsafe {
            ffmpeg::ffi::av_hwframe_get_buffer(self.frames_ctx, self.frame.as_mut_ptr(), 0)
        };
        if result < 0 {
            return Err(ffmpeg::Error::from(result)).context("failed to allocate VAAPI frame");
        }
        let result = unsafe {
            ffmpeg::ffi::av_hwframe_transfer_data(self.frame.as_mut_ptr(), source.as_ptr(), 0)
        };
        if result < 0 {
            return Err(ffmpeg::Error::from(result)).context("failed to upload frame to VAAPI");
        }
        let result =
            unsafe { ffmpeg::ffi::av_frame_copy_props(self.frame.as_mut_ptr(), source.as_ptr()) };
        if result < 0 {
            return Err(ffmpeg::Error::from(result))
                .context("failed to copy VAAPI frame properties");
        }
        Ok(&self.frame)
    }
}

impl Drop for VaapiUpload {
    fn drop(&mut self) {
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut self.frames_ctx) };
    }
}

struct AudioOutputStream {
    stream_index: usize,
    encoder: codec::encoder::audio::Encoder,
    resampler: Option<resampling::Context>,
}

struct SubtitleOutputStream {
    stream_index: usize,
}

impl EncodedOutput {
    pub(super) fn open_recording(
        cfg: &OutputConfig,
        recording_config: &crate::RecordingConfig,
    ) -> Result<Self> {
        let encode = recording_config
            .encode
            .as_ref()
            .context("dedicated recording encode settings are missing")?;
        let input_width = cfg.width;
        let input_height = cfg.height;
        let mut recording_cfg = cfg.clone();
        recording_cfg.width = encode.width.max(1);
        recording_cfg.height = encode.height.max(1);
        recording_cfg.video_codec = encode.video_codec.clone();
        recording_cfg.video_options = encode.video_options.clone();
        recording_cfg.audio_codec = encode.audio_codec.clone();
        recording_cfg.audio_options = encode.audio_options.clone();
        recording_cfg.audio_bitrate = encode.audio_bitrate;
        recording_cfg.audio_effects = crate::AudioEffectsControl::default();
        recording_cfg.audio_level_callback = None;
        recording_cfg.audio_frame_callback = None;
        recording_cfg.recording = None;
        let (pattern, monitor) = recording::prepare_recording(recording_config)?;
        let mut output = Self::open(
            pattern
                .to_str()
                .context("recording path is not valid UTF-8")?,
            &recording_cfg,
            EncodedFormat::Recording {
                segment_seconds: recording_config.segment_duration,
                input_width,
                input_height,
            },
        )?;
        output.recording_monitor = Some(monitor);
        Ok(output)
    }

    pub(super) fn open(
        path: &str,
        cfg: &OutputConfig,
        output_format: EncodedFormat,
    ) -> Result<Self> {
        Self::open_with_hls_health(path, cfg, output_format, None)
    }

    pub(super) fn open_with_hls_health(
        path: &str,
        cfg: &OutputConfig,
        output_format: EncodedFormat,
        hls_health: Option<HlsHealth>,
    ) -> Result<Self> {
        match &output_format {
            EncodedFormat::Hls { .. } => {
                validate_muxer_options("hls", &cfg.muxer_options).map_err(anyhow::Error::msg)?;
            }
            EncodedFormat::Stream { muxer } => {
                validate_muxer_options(muxer, &cfg.muxer_options).map_err(anyhow::Error::msg)?;
            }
            EncodedFormat::Auto | EncodedFormat::Recording { .. } => {}
        }
        let pace_output = !matches!(&output_format, EncodedFormat::Recording { .. });
        let hls_variants = match &output_format {
            EncodedFormat::Auto
            | EncodedFormat::Stream { .. }
            | EncodedFormat::Recording { .. } => &[][..],
            EncodedFormat::Hls { variants, .. } => variants.as_slice(),
        };
        let hls_subtitle = match &output_format {
            EncodedFormat::Hls { subtitle, .. } => subtitle.as_ref(),
            EncodedFormat::Auto
            | EncodedFormat::Stream { .. }
            | EncodedFormat::Recording { .. } => None,
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
            // The HLS muxer opens its playlists and segments itself. Avoid
            // preopening the output path because that truncates a standalone
            // playlist before `append_list` can resume it.
            EncodedFormat::Hls { .. } => hls::output_context(&hls_output_path)?,
            EncodedFormat::Stream { ref muxer } if is_network_url(path) => {
                format::output_as_with(path, muxer, network_io_options())?
            }
            EncodedFormat::Stream { ref muxer } => format::output_as(path, muxer)?,
            EncodedFormat::Recording { .. } => recording::segment_output_context(Path::new(path))?,
            EncodedFormat::Auto if path.starts_with("rtmp://") || path.starts_with("rtmps://") => {
                format::output_as_with(path, "flv", network_io_options())?
            }
            EncodedFormat::Auto if is_network_url(path) => {
                format::output_with(path, network_io_options())?
            }
            EncodedFormat::Auto => format::output(path)?,
        };
        // Matroska stores codec initialization data in its header. Keep it on
        // the shared encoders when a packet-copy recording is active; FFmpeg's
        // stream muxers accept those headers as well. An encode-mode recording
        // runs on its own independent encoders, so it must not affect the
        // primary output's header behavior.
        let global_header = cfg
            .recording
            .as_ref()
            .is_some_and(|recording| recording.encode.is_none())
            || octx
                .format()
                .flags()
                .contains(format::flag::Flags::GLOBAL_HEADER);

        let stream_count = hls_variants.len().max(1);
        let mut video_streams = Vec::with_capacity(stream_count);
        let mut audio_streams = Vec::with_capacity(stream_count);
        let mut subtitle_streams = Vec::with_capacity(usize::from(vtt_subtitles));

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
            EncodedFormat::Auto | EncodedFormat::Stream { .. } => {
                reject_unused_options(octx.write_header_with(muxer_options(&cfg.muxer_options))?)?;
            }
            EncodedFormat::Recording {
                segment_seconds, ..
            } => {
                let mut options = ffmpeg::Dictionary::new();
                options.set("segment_time", &segment_seconds.to_string());
                options.set("segment_format", "matroska");
                options.set("reset_timestamps", "1");
                options.set("strftime", "1");
                reject_unused_options(octx.write_header_with(options)?)?;
            }
            EncodedFormat::Hls {
                segment_seconds,
                list_size,
                ..
            } => {
                let default_hls_flags = if hls_start_number.is_some() {
                    "append_list+delete_segments+omit_endlist+temp_file+discont_start"
                } else {
                    "delete_segments+omit_endlist+temp_file"
                };
                let mut options = muxer_options_excluding(
                    &cfg.muxer_options,
                    [
                        "hls_time",
                        "hls_list_size",
                        "hls_flags",
                        "hls_segment_filename",
                        "start_number",
                        "master_pl_name",
                        "var_stream_map",
                    ],
                );
                options.set("hls_time", &segment_seconds.to_string());
                options.set("hls_list_size", &list_size.to_string());
                options.set(
                    "hls_flags",
                    &merged_hls_flags(default_hls_flags, cfg.muxer_options.get("hls_flags")),
                );
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

        let recording_video_stream_index = cfg
            .recording
            .as_ref()
            .map_or(0, |recording| recording.video_stream_index);
        let recording = if let Some(recording_config) = cfg
            .recording
            .as_ref()
            .filter(|recording| recording.encode.is_none())
        {
            match (|| -> Result<_> {
                let video_stream = video_streams
                    .get(recording_video_stream_index)
                    .context("recording video stream index is out of range")?;
                let audio_stream = audio_streams
                    .get(recording_video_stream_index)
                    .context("recording audio stream index is out of range")?;

                RecordingMuxer::open(
                    recording_config,
                    &video_stream.encoder,
                    &audio_stream.encoder,
                )
            })() {
                Ok(recording) => Some(recording),
                Err(error) => {
                    log::error!(channel = cfg.channel_id.unwrap_or_default(); "Recording disabled: {error}");
                    None
                }
            }
        } else {
            None
        };
        let transcoded_recording = if let Some(recording_config) = cfg.recording.as_ref()
            && recording_config.encode.is_some()
        {
            match Self::open_recording(cfg, recording_config).map(Box::new) {
                Ok(recording) => Some(recording),
                Err(error) => {
                    log::error!(channel = cfg.channel_id.unwrap_or_default(); "Recording disabled: {error}");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            octx,
            video_streams,
            audio_streams,
            subtitle_streams,
            vtt_subtitles,
            pending_vtt_cues: VecDeque::new(),
            audio_effects: AudioEffectChain::new(cfg.audio_effects.clone(), cfg.sample_rate),
            audio_level_meter: AudioLevelMeter::new(
                cfg.sample_rate,
                cfg.audio_level_callback.clone(),
            ),
            loudness_meter: LoudnessMeter::new(cfg.sample_rate, cfg.loudness_meter_control.clone()),
            audio_buffer: [VecDeque::new(), VecDeque::new()],
            audio_buffer_pts: None,
            audio_sample_rate: cfg.sample_rate,
            clock: PlayoutClock::new(),
            pace_output,
            hls_health,
            recording,
            recording_video_stream_index,
            transcoded_recording,
            recording_monitor: None,
            channel_id: cfg.channel_id,
        })
    }

    pub(super) fn audio_frame_size(&self) -> usize {
        self.audio_streams
            .first()
            .map(|stream| stream.encoder.frame_size() as usize)
            .unwrap_or(0)
    }

    pub(super) fn set_playout_rate(&mut self, rate: f64) {
        self.clock.set_rate(rate);
    }

    pub(super) fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        benchmark::measure(Stage::EncodeMux, || {
            for index in 0..self.video_streams.len() {
                let stream = &mut self.video_streams[index];
                let input_frame = if let Some(scaler) = &mut stream.scaler {
                    // The scaler allocates the destination using its output format.
                    // QSV and VAAPI need NV12 here, while software encoders use YUV420P.
                    let scaled_frame = stream.scaled_frame.get_or_insert_with(frame::Video::empty);
                    scaled_frame.set_pts(frame.pts());
                    scaler.run(frame, scaled_frame)?;
                    scaled_frame
                } else {
                    frame
                };
                if let Some(vaapi_upload) = &mut stream.vaapi_upload {
                    let vaapi_frame = vaapi_upload.upload(input_frame)?;
                    stream.encoder.send_frame(vaapi_frame)?;
                } else {
                    stream.encoder.send_frame(input_frame)?;
                }
                self.write_video_packets(index)?;
            }
            Ok::<_, anyhow::Error>(())
        })?;
        if let Some(error) = self
            .transcoded_recording
            .as_mut()
            .and_then(|recording| recording.encode_video(frame).err())
        {
            self.disable_transcoded_recording(error);
        }
        Ok(())
    }

    pub(super) fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        if frame.samples() == 0 {
            return Ok(());
        }

        let mut frame = frame.clone();
        benchmark::measure(Stage::AudioProcess, || {
            self.audio_effects.process(&mut frame);
            self.audio_level_meter.process_frame(&frame);
            self.loudness_meter.process_frame(&frame);
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
            Ok::<_, anyhow::Error>(())
        })?;

        self.write_complete_audio_frames()?;
        if let Some(error) = self
            .transcoded_recording
            .as_mut()
            .and_then(|recording| recording.encode_audio(&frame).err())
        {
            self.disable_transcoded_recording(error);
        }
        Ok(())
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
        self.pending_vtt_cues.clear();
        if !self.vtt_subtitles || self.subtitle_streams.is_empty() {
            return Ok(());
        }

        let vtt_path = vtt::sidecar_path(media_path);
        if !vtt_path.exists() {
            return Ok(());
        }

        let mut cues = vtt::parse_file(&vtt_path)?
            .into_iter()
            .filter(|cue| cue.end_ms > source_start_ms)
            .map(|cue| vtt::VttCue {
                start_ms: output_start_ms + cue.start_ms.saturating_sub(source_start_ms),
                end_ms: output_start_ms + cue.end_ms - source_start_ms,
                text: cue.text,
            })
            .collect::<Vec<_>>();
        cues.sort_by_key(|cue| cue.start_ms);
        self.pending_vtt_cues = cues.into();

        Ok(())
    }

    pub(super) fn clear_vtt_subtitles(&mut self) {
        self.pending_vtt_cues.clear();
    }

    pub(super) fn advance_vtt_subtitles(&mut self, output_position_ms: i64) -> Result<()> {
        while self
            .pending_vtt_cues
            .front()
            .is_some_and(|cue| cue.start_ms <= output_position_ms)
        {
            let cue = self.pending_vtt_cues.pop_front().expect("checked above");
            let mut packet = Packet::copy(cue.text.as_bytes());
            packet.set_pts(Some(cue.start_ms));
            packet.set_dts(Some(cue.start_ms));
            packet.set_duration(cue.end_ms - cue.start_ms);
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
        benchmark::measure(Stage::AudioEncode, || {
            for index in 0..self.audio_streams.len() {
                {
                    let stream = &mut self.audio_streams[index];
                    if let Some(resampler) = &mut stream.resampler {
                        let mut converted = frame::Audio::empty();
                        resampler.run(frame, &mut converted)?;
                        converted.set_pts(frame.pts());
                        stream.encoder.send_frame(&converted)?;
                    } else {
                        stream.encoder.send_frame(frame)?;
                    }
                }
                self.write_audio_packets(index)?;
            }
            Ok::<_, anyhow::Error>(())
        })
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
        if let Some(monitor) = &mut self.recording_monitor {
            monitor.check()?;
        }
        let stream_time_base = self
            .octx
            .stream(stream_index)
            .context("output stream is missing")?
            .time_base();

        packet.set_stream(stream_index);
        packet.rescale_ts(encoder_time_base, stream_time_base);
        if self.pace_output {
            self.clock
                .wait_until(packet.dts().or_else(|| packet.pts()), stream_time_base);
        }
        packet.write_interleaved(&mut self.octx)?;
        if let Some(health) = &self.hls_health {
            health.mark_muxed();
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<()> {
        benchmark::measure(Stage::AudioEncode, || {
            self.pad_audio_buffer()?;
            for index in 0..self.audio_streams.len() {
                self.flush_audio_resampler(index)?;
                self.audio_streams[index].encoder.send_eof()?;
                self.write_audio_packets(index)?;
            }
            Ok::<_, anyhow::Error>(())
        })?;

        benchmark::measure(Stage::EncodeMux, || {
            for index in 0..self.video_streams.len() {
                self.video_streams[index].encoder.send_eof()?;
                self.write_video_packets(index)?;
            }

            self.octx.write_trailer()?;
            if let Some(recording) = self.recording.take()
                && let Err(error) = recording.finish()
            {
                self.log_recording_error(error);
            }
            Ok::<_, anyhow::Error>(())
        })?;
        if let Some(recording) = self.transcoded_recording.take()
            && let Err(error) = recording.finish()
        {
            self.log_recording_error(error);
        }
        Ok(())
    }

    fn flush_audio_resampler(&mut self, index: usize) -> Result<()> {
        loop {
            let delay = {
                let stream = &mut self.audio_streams[index];
                let Some(resampler) = &mut stream.resampler else {
                    return Ok(());
                };
                let output = *resampler.output();
                let mut converted = frame::Audio::new(
                    output.format,
                    stream.encoder.frame_size() as usize,
                    output.channel_layout,
                );
                converted.set_rate(output.rate);
                let delay = resampler.flush(&mut converted)?;

                if converted.samples() > 0 {
                    stream.encoder.send_frame(&converted)?;
                }
                delay
            };
            self.write_audio_packets(index)?;

            if delay.is_none() {
                return Ok(());
            }
        }
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
            let recording_error = (index == self.recording_video_stream_index)
                .then(|| {
                    self.recording
                        .as_mut()
                        .and_then(|recording| recording.write_video(&packet, time_base).err())
                })
                .flatten();
            if let Some(error) = recording_error {
                self.disable_copy_recording(error);
            }
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
            let recording_error = (index == self.recording_video_stream_index)
                .then(|| {
                    self.recording
                        .as_mut()
                        .and_then(|recording| recording.write_audio(&packet, time_base).err())
                })
                .flatten();
            if let Some(error) = recording_error {
                self.disable_copy_recording(error);
            }
            self.write_packet(&mut packet, stream_index, time_base)?;
        }
        Ok(())
    }

    fn disable_copy_recording(&mut self, error: anyhow::Error) {
        self.recording.take();
        self.log_recording_error(error);
    }

    fn disable_transcoded_recording(&mut self, error: anyhow::Error) {
        self.transcoded_recording.take();
        self.log_recording_error(error);
    }

    fn log_recording_error(&self, error: impl std::fmt::Display) {
        log::error!(channel = self.channel_id.unwrap_or_default(); "Recording disabled: {error}");
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

fn muxer_options(options: &BTreeMap<String, String>) -> ffmpeg::Dictionary<'static> {
    muxer_options_excluding(options, [])
}

fn muxer_options_excluding<'a>(
    options: &BTreeMap<String, String>,
    excluded: impl IntoIterator<Item = &'a str>,
) -> ffmpeg::Dictionary<'static> {
    let excluded = excluded.into_iter().collect::<BTreeSet<_>>();
    let mut dictionary = ffmpeg::Dictionary::new();
    for (key, value) in options {
        if !excluded.contains(key.as_str()) {
            dictionary.set(key, value);
        }
    }
    dictionary
}

fn merged_hls_flags(default_flags: &str, configured_flags: Option<&String>) -> String {
    let required = default_flags
        .split('+')
        .filter(|flag| !flag.is_empty())
        .collect::<BTreeSet<_>>();
    let mut flags = required.iter().copied().collect::<Vec<_>>();

    for flag in configured_flags
        .into_iter()
        .flat_map(|value| value.split('+'))
        .filter(|flag| !flag.is_empty())
    {
        // ffmpeg supports `-flag` to remove a flag. Never let a configured
        // value turn off a flag that ffplayout needs for its HLS lifecycle.
        if required.contains(flag)
            || flag
                .strip_prefix('-')
                .is_some_and(|flag| required.contains(flag))
        {
            continue;
        }
        if !flags.contains(&flag) {
            flags.push(flag);
        }
    }
    flags.join("+")
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
    let video_codec = if cfg.video_codec.trim().is_empty() {
        codec::encoder::find(codec::Id::H264)
    } else {
        codec::encoder::find_by_name(cfg.video_codec.trim())
    }
    .with_context(|| format!("video encoder {:?} not found", cfg.video_codec))?;
    let mut video_stream = octx.add_stream(video_codec)?;
    let mut video_ctx = codec::context::Context::new_with_codec(video_codec)
        .encoder()
        .video()?;
    let encoder_backend = VideoEncoderBackend::from_name(video_codec.name());
    let encoder_format = encoder_backend.encoder_format();
    let cpu_input_format = encoder_backend.cpu_input_format();
    let width = variant.map_or(cfg.width, |variant| variant.width);
    let height = variant.map_or(cfg.height, |variant| variant.height);
    let (input_width, input_height) = match &output_format {
        EncodedFormat::Recording {
            input_width,
            input_height,
            ..
        } => (*input_width, *input_height),
        _ => (cfg.width, cfg.height),
    };
    video_ctx.set_width(width);
    video_ctx.set_height(height);
    video_ctx.set_format(encoder_format);
    video_ctx.set_time_base(cfg.video_time_base);
    video_ctx.set_frame_rate(Some(Rational(cfg.fps as i32, 1)));
    let maxrate = variant.map_or(cfg.video_maxrate(), |variant| variant.video_bitrate);
    if encoder_backend.uses_target_bitrate(cfg, video_codec) {
        video_ctx.set_bit_rate(maxrate as usize);
    }
    match &output_format {
        EncodedFormat::Hls {
            segment_seconds, ..
        } => video_ctx.set_gop(hls_gop_size(cfg.fps, *segment_seconds)),
        EncodedFormat::Stream { .. } | EncodedFormat::Recording { .. } => {
            video_ctx.set_gop(stream_gop_size(cfg.fps));
        }
        EncodedFormat::Auto => {}
    }
    let mut video_flags = codec::flag::Flags::empty();
    if global_header {
        video_flags |= codec::flag::Flags::GLOBAL_HEADER;
    }
    if matches!(output_format, EncodedFormat::Hls { .. }) {
        video_flags |= codec::flag::Flags::CLOSED_GOP;
    }
    if encoder_backend == VideoEncoderBackend::Qsv && qsv_uses_icq(cfg) {
        // QSV selects ICQ from AVCodecContext::global_quality. Passing this
        // through the encoder option dictionary does not reliably update the
        // context before rate control is selected.
        let global_quality = qsv_global_quality(cfg);
        log::debug!("QSV encoder rate control: ICQ, global quality: {global_quality}");
        video_ctx.set_global_quality(global_quality);
    }
    if !video_flags.is_empty() {
        video_ctx.set_flags(video_flags);
    }

    let vaapi_upload = (encoder_backend == VideoEncoderBackend::Vaapi)
        .then(|| VaapiUpload::new(width, height))
        .transpose()?;
    if let Some(vaapi_upload) = &vaapi_upload {
        vaapi_upload.attach_to_encoder(&mut video_ctx)?;
    }

    let mut options = ffmpeg::Dictionary::new();
    encoder_backend.configure_options(&mut options, cfg, maxrate);

    let mut video_encoder = match output_format {
        EncodedFormat::Auto | EncodedFormat::Stream { .. } | EncodedFormat::Recording { .. } => {
            video_ctx.open_as_with(video_codec, options)?
        }
        EncodedFormat::Hls { .. } => {
            if encoder_backend == VideoEncoderBackend::X264 {
                options.set("x264-params", "open-gop=0:repeat-headers=1");
            }
            video_ctx.open_as_with(video_codec, options)?
        }
    };
    // Quality-based encoders clear AVCodecContext::bit_rate while opening.
    // Restore the configured maximum as metadata so the HLS muxer can
    // calculate BANDWIDTH for master.m3u8. QSV and VAAPI observe a bitrate
    // change when their quality-only mode begins, which would invalidate it.
    if video_codec_uses_bitrate(video_codec.name())
        && restore_video_bitrate_metadata(encoder_backend, cfg)
    {
        video_encoder.set_bit_rate(maxrate as usize);
    }
    video_stream.set_parameters(&video_encoder);
    video_stream.set_time_base(cfg.video_time_base);
    let stream_index = video_stream.index();
    let scaler =
        if width == input_width && height == input_height && cpu_input_format == Pixel::YUV420P {
            None
        } else {
            Some(scaling::Context::get(
                Pixel::YUV420P,
                input_width,
                input_height,
                cpu_input_format,
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
        vaapi_upload,
    })
}

/// Encoders in this pipeline receive CPU-backed frames. QSV accepts NV12 and
/// uploads it internally; VAAPI receives an NV12 frame uploaded to a VAAPI
/// frame pool immediately before encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoEncoderBackend {
    Software,
    X264,
    X265,
    Nvenc,
    Qsv,
    Vaapi,
    VpxVp9,
    SvtAv1,
}

impl VideoEncoderBackend {
    fn from_name(name: &str) -> Self {
        if name.contains("x264") {
            Self::X264
        } else if name.contains("x265") {
            Self::X265
        } else if matches!(name, "h264_nvenc" | "hevc_nvenc" | "av1_nvenc") {
            Self::Nvenc
        } else if matches!(name, "h264_qsv" | "hevc_qsv" | "av1_qsv") {
            Self::Qsv
        } else if matches!(name, "h264_vaapi" | "hevc_vaapi" | "av1_vaapi") {
            Self::Vaapi
        } else if name == "libvpx-vp9" {
            Self::VpxVp9
        } else if name == "libsvtav1" {
            Self::SvtAv1
        } else {
            Self::Software
        }
    }

    const fn encoder_format(self) -> Pixel {
        match self {
            Self::Vaapi => Pixel::VAAPI,
            _ => self.cpu_input_format(),
        }
    }

    const fn cpu_input_format(self) -> Pixel {
        match self {
            Self::Qsv => Pixel::NV12,
            Self::Vaapi => Pixel::NV12,
            Self::Software
            | Self::X264
            | Self::X265
            | Self::Nvenc
            | Self::VpxVp9
            | Self::SvtAv1 => Pixel::YUV420P,
        }
    }

    fn uses_target_bitrate(self, cfg: &OutputConfig, codec: codec::codec::Codec) -> bool {
        cfg.video_option("rate_control") == Some("cbr")
            || self == Self::VpxVp9
            || (self == Self::Vaapi && cfg.video_option("rate_control") == Some("vbr"))
            // Encoders that fall through to the generic Software backend and
            // expose none of the rate-control AVOptions this pipeline sets
            // (maxrate/bufsize/crf/qp) only understand a plain target
            // bitrate via AVCodecContext::bit_rate. Detecting this
            // capability at runtime - instead of matching specific codec
            // names such as "h264_v4l2m2m" - covers every current and
            // future encoder with the same limitation (other stateless HW
            // wrappers included) without growing a per-codec list.
            || (self == Self::Software && video_encoder_is_bitrate_only(codec))
    }

    fn configure_options(
        self,
        options: &mut ffmpeg::Dictionary<'_>,
        cfg: &OutputConfig,
        maxrate: u64,
    ) {
        match self {
            Self::X264 | Self::X265 => {
                options.set("preset", cfg.video_option("preset").unwrap_or("faster"));
                options.set("tune", "zerolatency");
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());
                match cfg.video_option("rate_control") {
                    Some("cbr") => options.set("minrate", &maxrate.to_string()),
                    _ => options.set("crf", cfg.video_option("quality").unwrap_or("23")),
                }
            }
            Self::Nvenc => {
                options.set("preset", "p4");
                options.set("tune", "ll");
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());
                match cfg.video_option("rate_control") {
                    Some("cbr") => options.set("rc", "cbr"),
                    _ => {
                        options.set("rc", "vbr");
                        options.set("cq", cfg.video_option("quality").unwrap_or("23"));
                    }
                }
            }
            Self::Qsv => {
                options.set("preset", cfg.video_option("preset").unwrap_or("faster"));
                options.set("async_depth", "4");
                options.set("low_delay_brc", "1");
                if !qsv_uses_icq(cfg) {
                    options.set("maxrate", &maxrate.to_string());
                    options.set("bufsize", &maxrate.saturating_mul(2).to_string());
                    if cfg.video_option("rate_control") == Some("cbr") {
                        options.set("rdo", "0");
                    }
                }
            }
            Self::Vaapi => {
                let rate_control = cfg.video_option("rate_control").unwrap_or("vbr");
                options.set("async_depth", "4");
                options.set(
                    "rc_mode",
                    match rate_control {
                        "cqp" => "CQP",
                        "cbr" => "CBR",
                        _ => "VBR",
                    },
                );
                if rate_control == "cqp" {
                    options.set("qp", cfg.video_option("quality").unwrap_or("23"));
                } else {
                    options.set("maxrate", &maxrate.to_string());
                    options.set("bufsize", &maxrate.saturating_mul(2).to_string());
                }
            }
            Self::VpxVp9 => {
                options.set("deadline", cfg.video_option("deadline").unwrap_or("good"));
                options.set("cpu-used", cfg.video_option("cpu-used").unwrap_or("4"));
                options.set("row-mt", cfg.video_option("row-mt").unwrap_or("auto"));
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());
                match cfg.video_option("rate_control") {
                    Some("cbr") => options.set("minrate", &maxrate.to_string()),
                    _ => options.set("crf", cfg.video_option("quality").unwrap_or("31")),
                }
            }
            Self::SvtAv1 => {
                options.set("preset", cfg.video_option("preset").unwrap_or("8"));
                options.set("crf", cfg.video_option("quality").unwrap_or("30"));
                options.set("maxrate", &maxrate.to_string());
                options.set("bufsize", &maxrate.saturating_mul(2).to_string());
            }
            Self::Software => {}
        }
    }
}

/// Checks whether an encoder's private option class (`AVCodec::priv_class`)
/// declares the given option name, without needing to open/instantiate the
/// encoder. This mirrors what `ffmpeg -h encoder=<name>` shows under
/// "AVOptions" and is the generic, ffmpeg-native way to introspect what an
/// encoder actually supports at runtime.
fn video_encoder_has_option(codec: codec::codec::Codec, name: &str) -> bool {
    let Ok(name) = CString::new(name) else {
        return false;
    };

    unsafe {
        let priv_class = (*codec.as_ptr()).priv_class;
        if priv_class.is_null() {
            return false;
        }

        !ffmpeg::ffi::av_opt_find(
            ptr::from_ref(&priv_class).cast_mut().cast(),
            name.as_ptr(),
            ptr::null(),
            0,
            ffmpeg::ffi::AV_OPT_SEARCH_FAKE_OBJ,
        )
        .is_null()
    }
}

/// Some encoders (most notably stateless HW wrapper encoders like the V4L2
/// M2M drivers used on Raspberry Pi / embedded SoCs) expose none of the
/// rate-control AVOptions this pipeline normally configures (maxrate,
/// bufsize, crf, qp). They only honor a plain target bitrate set on
/// `AVCodecContext::bit_rate`, otherwise the underlying driver falls back
/// to its own default (observed around 300-400kbps on a Raspberry Pi 4).
/// Detecting this by capability instead of by codec name means any encoder
/// with the same limitation - present today or added to ffmpeg later - is
/// handled automatically, without maintaining a per-codec list here.
fn video_encoder_is_bitrate_only(codec: codec::codec::Codec) -> bool {
    ["maxrate", "bufsize", "crf", "qp"]
        .iter()
        .all(|option| !video_encoder_has_option(codec, option))
}

fn qsv_uses_icq(cfg: &OutputConfig) -> bool {
    cfg.video_option("rate_control") == Some("icq")
}

fn qsv_global_quality(cfg: &OutputConfig) -> i32 {
    cfg.video_option("global_quality")
        .and_then(|value| value.parse().ok())
        .filter(|value| (1..=51).contains(value))
        .unwrap_or(23)
}

fn restore_video_bitrate_metadata(backend: VideoEncoderBackend, cfg: &OutputConfig) -> bool {
    !(backend == VideoEncoderBackend::Qsv && qsv_uses_icq(cfg)
        || backend == VideoEncoderBackend::Vaapi && cfg.video_option("rate_control") == Some("cqp"))
}

/// Use a keyframe interval that fits exactly into the requested HLS segment.
/// Keeping it at two seconds or less balances segment precision and bitrate.
fn hls_gop_size(fps: u32, segment_seconds: u32) -> u32 {
    let segment_seconds = segment_seconds.max(1);
    let gop_seconds = (1..=segment_seconds.min(2))
        .rev()
        .find(|seconds| segment_seconds.is_multiple_of(*seconds))
        .unwrap_or(1);

    fps.saturating_mul(gop_seconds)
}

fn stream_gop_size(fps: u32) -> u32 {
    fps.saturating_mul(2)
}

fn open_audio_stream(
    octx: &mut format::context::Output,
    cfg: &OutputConfig,
    global_header: bool,
    variant: Option<&HlsVariant>,
) -> Result<AudioOutputStream> {
    let audio_codec = if cfg.audio_codec.trim().is_empty() {
        codec::encoder::find(codec::Id::AAC)
    } else {
        codec::encoder::find_by_name(cfg.audio_codec.trim())
    }
    .with_context(|| format!("audio encoder {:?} not found", cfg.audio_codec))?;
    let mut audio_stream = octx.add_stream(audio_codec)?;
    let audio_ctx = audio_encoder_context(
        audio_codec,
        &cfg.audio_options,
        cfg.sample_rate,
        variant.map_or(cfg.audio_bitrate, |variant| variant.audio_bitrate),
        cfg.audio_time_base,
        global_header,
    )
    .map_err(anyhow::Error::msg)?;
    let input_sample_format = engine_audio_sample_format();
    let encoder_sample_format = audio_ctx.format();
    let audio_encoder = audio_ctx.open_as(audio_codec)?;
    audio_stream.set_parameters(&audio_encoder);
    audio_stream.set_time_base(cfg.audio_time_base);
    let resampler = (encoder_sample_format != input_sample_format)
        .then(|| {
            resampling::Context::get(
                input_sample_format,
                ChannelLayout::STEREO,
                cfg.sample_rate,
                encoder_sample_format,
                ChannelLayout::STEREO,
                cfg.sample_rate,
            )
        })
        .transpose()?;
    Ok(AudioOutputStream {
        stream_index: audio_stream.index(),
        encoder: audio_encoder,
        resampler,
    })
}

fn open_subtitle_stream(octx: &mut format::context::Output) -> Result<SubtitleOutputStream> {
    let mut stream = octx.add_stream(codec::Id::WEBVTT)?;
    stream.set_time_base(Rational(1, 1_000));
    let mut parameters = codec::Parameters::new();
    parameters.set_medium(ffmpeg::media::Type::Subtitle);
    parameters.set_id(codec::Id::WEBVTT);
    stream.set_parameters(parameters);
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
    fn configured_hls_flags_are_combined_with_required_flags() {
        let flags = merged_hls_flags(
            "delete_segments+omit_endlist+temp_file",
            Some(&"program_date_time+temp_file".to_string()),
        );

        assert_eq!(
            flags,
            "delete_segments+omit_endlist+temp_file+program_date_time"
        );
    }

    #[test]
    fn configured_hls_flags_cannot_disable_required_flags() {
        let flags = merged_hls_flags("delete_segments+temp_file", Some(&"-temp_file".to_string()));

        assert_eq!(flags, "delete_segments+temp_file");
    }

    #[test]
    fn hls_accepts_program_date_time_muxer_option() {
        ffmpeg::init().ok();
        let dir =
            std::env::temp_dir().join(format!("hls_program_date_time_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stream.m3u8");
        let cfg = OutputConfig::new(320, 240, 25, 44_100).with_muxer_options(BTreeMap::from([(
            "hls_flags".to_string(),
            "program_date_time".to_string(),
        )]));

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
        .expect("expected hls_flags=program_date_time to be accepted");

        for index in 0..50 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            video.set_pts(Some(index));
            video.data_mut(0).fill(16);
            video.data_mut(1).fill(128);
            video.data_mut(2).fill(128);
            output.encode_video(&video).unwrap();

            let mut audio = frame::Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Planar),
                output.audio_frame_size(),
                ChannelLayout::STEREO,
            );
            audio.set_rate(44_100);
            audio.set_pts(Some(index * output.audio_frame_size() as i64));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            output.encode_audio(&audio).unwrap();
        }
        output.finish().unwrap();

        let playlist = fs::read_to_string(&path).unwrap();
        assert!(playlist.contains("#EXT-X-PROGRAM-DATE-TIME:"), "{playlist}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn required_hls_options_are_not_passed_through_as_custom_options() {
        let options = BTreeMap::from([
            ("hls_time".to_string(), "999".to_string()),
            ("hls_start_number_source".to_string(), "epoch".to_string()),
        ]);
        let dictionary = muxer_options_excluding(&options, ["hls_time"]);

        assert_eq!(dictionary.get("hls_time"), None);
        assert_eq!(dictionary.get("hls_start_number_source"), Some("epoch"));
    }

    #[test]
    fn selects_nvenc_for_cpu_backed_hardware_encoding() {
        assert_eq!(
            VideoEncoderBackend::from_name("h264_nvenc"),
            VideoEncoderBackend::Nvenc
        );
        assert_eq!(
            VideoEncoderBackend::from_name("hevc_nvenc"),
            VideoEncoderBackend::Nvenc
        );
        assert_eq!(
            VideoEncoderBackend::from_name("libx264"),
            VideoEncoderBackend::X264
        );
        assert_eq!(
            VideoEncoderBackend::from_name("libx265"),
            VideoEncoderBackend::X265
        );
        assert_eq!(
            VideoEncoderBackend::from_name("h264_qsv"),
            VideoEncoderBackend::Qsv
        );
        assert_eq!(VideoEncoderBackend::Qsv.cpu_input_format(), Pixel::NV12);
        assert_eq!(
            VideoEncoderBackend::from_name("h264_vaapi"),
            VideoEncoderBackend::Vaapi
        );
        assert_eq!(VideoEncoderBackend::Vaapi.cpu_input_format(), Pixel::NV12);
        assert_eq!(VideoEncoderBackend::Vaapi.encoder_format(), Pixel::VAAPI);
        assert_eq!(
            VideoEncoderBackend::from_name("mpeg4"),
            VideoEncoderBackend::Software
        );
    }

    #[test]
    fn bitrate_only_encoders_are_detected_without_naming_them() {
        // h264_v4l2m2m (and other stateless V4L2 M2M wrappers on e.g.
        // Raspberry Pi) expose no maxrate/bufsize/crf/qp AVOptions - only a
        // plain AVCodecContext::bit_rate is honored. This must be detected
        // by capability probing, not by matching "_v4l2m2m" or any other
        // codec name, so any encoder with the same limitation is covered.
        let Some(codec) = codec::encoder::find_by_name("h264_v4l2m2m") else {
            return;
        };

        assert_eq!(
            VideoEncoderBackend::from_name(codec.name()),
            VideoEncoderBackend::Software
        );
        assert!(video_encoder_is_bitrate_only(codec));

        let cfg = OutputConfig::new(320, 240, 25, 44_100);
        assert!(VideoEncoderBackend::Software.uses_target_bitrate(&cfg, codec));
    }

    #[test]
    fn encoders_with_rate_control_avoptions_are_not_bitrate_only() {
        let codec = codec::encoder::find(codec::Id::H264).expect("libx264 is required for tests");

        assert!(!video_encoder_is_bitrate_only(codec));

        let cfg = OutputConfig::new(320, 240, 25, 44_100);
        assert!(!VideoEncoderBackend::X264.uses_target_bitrate(&cfg, codec));
    }

    #[test]
    fn qsv_input_scaler_allocates_nv12_output() {
        let mut scaler = scaling::Context::get(
            Pixel::YUV420P,
            320,
            240,
            Pixel::NV12,
            320,
            240,
            scaling::flag::Flags::BILINEAR,
        )
        .unwrap();
        let input = frame::Video::new(Pixel::YUV420P, 320, 240);
        let mut output = frame::Video::empty();

        scaler.run(&input, &mut output).unwrap();

        assert_eq!(output.format(), Pixel::NV12);
        assert_eq!((output.width(), output.height()), (320, 240));
    }

    #[test]
    fn vaapi_uses_nv12_before_hardware_upload() {
        let mut scaler = scaling::Context::get(
            Pixel::YUV420P,
            320,
            240,
            VideoEncoderBackend::Vaapi.cpu_input_format(),
            320,
            240,
            scaling::flag::Flags::BILINEAR,
        )
        .unwrap();
        let input = frame::Video::new(Pixel::YUV420P, 320, 240);
        let mut output = frame::Video::empty();

        scaler.run(&input, &mut output).unwrap();

        assert_eq!(output.format(), Pixel::NV12);
    }

    #[test]
    fn qsv_icq_sets_global_quality_on_the_codec_context() {
        let mut cfg = OutputConfig::new(320, 240, 25, 44_100);
        cfg.video_options
            .insert("rate_control".into(), "icq".into());
        cfg.video_options
            .insert("global_quality".into(), "17".into());

        assert!(qsv_uses_icq(&cfg));
        assert_eq!(qsv_global_quality(&cfg), 17);
        assert!(!restore_video_bitrate_metadata(
            VideoEncoderBackend::Qsv,
            &cfg
        ));

        cfg.video_options
            .insert("global_quality".into(), "invalid".into());
        assert_eq!(qsv_global_quality(&cfg), 23);

        cfg.video_options
            .insert("rate_control".into(), "vbr".into());
        assert!(restore_video_bitrate_metadata(
            VideoEncoderBackend::Qsv,
            &cfg
        ));
    }

    #[test]
    fn vaapi_cqp_does_not_restore_bitrate_metadata() {
        let mut cfg = OutputConfig::new(320, 240, 25, 44_100);
        cfg.video_options
            .insert("rate_control".into(), "cqp".into());

        assert!(!restore_video_bitrate_metadata(
            VideoEncoderBackend::Vaapi,
            &cfg
        ));
    }

    #[test]
    fn hls_gop_is_a_short_divisor_of_the_segment_duration() {
        assert_eq!(hls_gop_size(25, 6), 50);
        assert_eq!(hls_gop_size(25, 5), 25);
        assert_eq!(hls_gop_size(30, 4), 60);
        assert_eq!(hls_gop_size(50, 1), 50);
    }

    #[test]
    fn stream_gop_is_two_seconds() {
        assert_eq!(stream_gop_size(25), 50);
        assert_eq!(stream_gop_size(30), 60);
    }

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
    fn hls_vtt_resume_after_live_does_not_write_future_cues_twice() {
        ffmpeg::init().ok();
        let dir =
            std::env::temp_dir().join(format!("hls_vtt_live_resume_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.m3u8");
        let media_path = dir.join("programme.mp4");
        fs::write(
            media_path.with_extension("vtt"),
            "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nFirst\n\n00:00:50.000 --> 00:00:51.000\nFuture\n",
        )
        .unwrap();
        let cfg = OutputConfig::new(320, 240, 25, 44_100);
        let mut output = EncodedOutput::open(
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
        )
        .unwrap();

        // Only the first cue is reached before live takeover interrupts the
        // clip. The cue at source second 50 must remain outside the muxer.
        output
            .write_vtt_subtitles(media_path.to_str().unwrap(), 1_000_000, 0)
            .unwrap();
        output.advance_vtt_subtitles(1_001_500).unwrap();
        assert_eq!(output.pending_vtt_cues.len(), 1);

        // Forty seconds of live output pass, then the same clip resumes at
        // source second 40. Its remaining cue must keep increasing DTS.
        output
            .write_vtt_subtitles(media_path.to_str().unwrap(), 1_040_000, 40_000)
            .unwrap();
        assert_eq!(output.pending_vtt_cues.len(), 1);
        output.advance_vtt_subtitles(1_050_000).unwrap();
        assert!(output.pending_vtt_cues.is_empty());
        output.finish().unwrap();
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
            "libx264".to_string(),
            [
                ("preset".to_string(), "faster".to_string()),
                ("rate_control".to_string(), "cbr".to_string()),
                ("quality".to_string(), "23".to_string()),
                ("maxrate".to_string(), "1300".to_string()),
            ]
            .into_iter()
            .collect(),
            "aac".to_string(),
            BTreeMap::from([
                ("aac_coder".to_string(), "fast".to_string()),
                ("cutoff".to_string(), "18000".to_string()),
            ]),
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
    fn stream_output_can_use_mpegts_muxer() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("stream_mpegts_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stream.ts");
        let output = EncodedOutput::open(
            path.to_str().unwrap(),
            &OutputConfig::new(320, 240, 25, 44100),
            EncodedFormat::Stream {
                muxer: "mpegts".to_string(),
            },
        )
        .unwrap();

        output.finish().unwrap();
        assert!(path.exists(), "expected stream.ts to exist");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stream_output_can_open_segmented_matroska_recording() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("recording_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let stream_path = dir.join("stream.ts");
        let recording_path = dir.join("recording");
        let cfg = OutputConfig::new(320, 240, 25, 44_100).with_recording(Some(
            crate::RecordingConfig::new(recording_path.to_string_lossy(), 300),
        ));

        let mut output = EncodedOutput::open(
            stream_path.to_str().unwrap(),
            &cfg,
            EncodedFormat::Stream {
                muxer: "mpegts".to_string(),
            },
        )
        .unwrap();

        for index in 0..25 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            video.set_pts(Some(index));
            video.data_mut(0).fill(16);
            output.encode_video(&video).unwrap();

            let mut audio = frame::Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Planar),
                output.audio_frame_size(),
                ChannelLayout::STEREO,
            );
            audio.set_rate(44_100);
            audio.set_pts(Some(index * output.audio_frame_size() as i64));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            output.encode_audio(&audio).unwrap();
        }

        output.finish().unwrap();
        assert!(
            recording_path.exists(),
            "expected recording directory to exist"
        );
        assert!(
            fs::read_dir(&recording_path).unwrap().next().is_some(),
            "expected a recording segment"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recording_open_failure_does_not_stop_primary_output() {
        ffmpeg::init().ok();
        let dir =
            std::env::temp_dir().join(format!("recording_failure_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let blocked_path = dir.join("not-a-directory");
        fs::write(&blocked_path, "file").unwrap();
        let cfg = OutputConfig::new(320, 240, 25, 44_100).with_recording(Some(
            crate::RecordingConfig::new(blocked_path.to_string_lossy(), 300),
        ));

        let output = EncodedOutput::open(
            dir.join("stream.ts").to_str().unwrap(),
            &cfg,
            EncodedFormat::Stream {
                muxer: "mpegts".to_string(),
            },
        );

        assert!(
            output.is_ok(),
            "recording errors must not stop the primary output"
        );
        output.unwrap().finish().unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hls_output_can_copy_to_segmented_matroska_recording() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("hls_recording_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let recording_path = dir.join("recording");
        let cfg = OutputConfig::new(320, 240, 25, 44_100).with_recording(Some(
            crate::RecordingConfig::new(recording_path.to_string_lossy(), 300),
        ));
        let mut output = EncodedOutput::open(
            dir.join("stream.m3u8").to_str().unwrap(),
            &cfg,
            EncodedFormat::Hls {
                variants: vec![],
                subtitle: None,
                segment_seconds: 1,
                list_size: 60,
            },
        )
        .unwrap();

        for index in 0..25 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            video.set_pts(Some(index));
            video.data_mut(0).fill(16);
            video.data_mut(1).fill(128);
            video.data_mut(2).fill(128);
            output.encode_video(&video).unwrap();
            let mut audio = frame::Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Planar),
                output.audio_frame_size(),
                ChannelLayout::STEREO,
            );
            audio.set_rate(44_100);
            audio.set_pts(Some(index * output.audio_frame_size() as i64));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            output.encode_audio(&audio).unwrap();
        }
        output.finish().unwrap();
        let segments = fs::read_dir(&recording_path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!segments.is_empty());
        assert!(segments.iter().all(|name| !name.contains('%')));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stream_output_can_transcode_segmented_matroska_recording() {
        ffmpeg::init().ok();
        let dir =
            std::env::temp_dir().join(format!("recording_encode_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let recording_path = dir.join("recording");
        let recording = crate::RecordingConfig::new(recording_path.to_string_lossy(), 300)
            .with_encode(crate::RecordingEncodeConfig {
                width: 160,
                height: 120,
                video_codec: "libx264".to_string(),
                video_options: crate::video_option_defaults("libx264"),
                audio_codec: "aac".to_string(),
                audio_options: BTreeMap::new(),
                audio_bitrate: 96_000,
            });
        let cfg = OutputConfig::new(320, 240, 25, 44_100).with_recording(Some(recording));
        let mut output = EncodedOutput::open(
            dir.join("stream.ts").to_str().unwrap(),
            &cfg,
            EncodedFormat::Stream {
                muxer: "mpegts".to_string(),
            },
        )
        .unwrap();

        let recording_scaler = output
            .transcoded_recording
            .as_ref()
            .and_then(|recording| recording.video_streams.first())
            .and_then(|stream| stream.scaler.as_ref())
            .expect("recording resolution should require a scaler");
        assert_eq!(
            (
                recording_scaler.input().width,
                recording_scaler.input().height
            ),
            (320, 240)
        );
        assert_eq!(
            (
                recording_scaler.output().width,
                recording_scaler.output().height
            ),
            (160, 120)
        );

        for index in 0..25 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            video.set_pts(Some(index));
            video.data_mut(0).fill(16);
            video.data_mut(1).fill(128);
            video.data_mut(2).fill(128);
            output.encode_video(&video).unwrap();
            let mut audio = frame::Audio::new(
                Sample::F32(ffmpeg::format::sample::Type::Planar),
                output.audio_frame_size(),
                ChannelLayout::STEREO,
            );
            audio.set_rate(44_100);
            audio.set_pts(Some(index * output.audio_frame_size() as i64));
            for channel in 0..2 {
                audio.plane_mut::<f32>(channel).fill(0.0);
            }
            output.encode_audio(&audio).unwrap();
        }
        output.finish().unwrap();
        let segments = fs::read_dir(&recording_path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!segments.is_empty());
        assert!(segments.iter().all(|name| !name.contains('%')));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn libfdk_aac_sample_format_is_converted_when_available() {
        ffmpeg::init().ok();
        if codec::encoder::find_by_name("libfdk_aac").is_none() {
            return;
        }

        let dir = std::env::temp_dir().join(format!("hls_fdk_aac_test_{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stream.m3u8");
        let cfg = OutputConfig::new(320, 240, 25, 44100).with_encoding(
            "libx264".to_string(),
            [
                ("preset".to_string(), "faster".to_string()),
                ("rate_control".to_string(), "crf".to_string()),
                ("quality".to_string(), "23".to_string()),
                ("maxrate".to_string(), "1300".to_string()),
            ]
            .into_iter()
            .collect(),
            "libfdk_aac".to_string(),
            BTreeMap::new(),
            128_000,
        );
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

        for index in 0..25 {
            let mut video = frame::Video::new(Pixel::YUV420P, 320, 240);
            video.set_pts(Some(index));
            video.data_mut(0).fill(16);
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
        assert!(path.exists(), "expected stream.m3u8 to exist");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn standalone_hls_resumes_existing_playlist() {
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
        assert!(playlist.contains("stream_0.ts"), "{playlist}");
        assert!(playlist.contains("stream_2.ts"), "{playlist}");
        assert_eq!(
            fs::read(dir.join("stream_0.ts")).unwrap(),
            fs::read(dir.join("stream_0.snapshot")).unwrap()
        );
        assert!(dir.join("stream_2.ts").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resumed_hls_deletes_segments_that_leave_playlist() {
        ffmpeg::init().ok();
        let dir = std::env::temp_dir().join(format!("hls_delete_test_{}", std::process::id()));
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
                    list_size: 2,
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
        }

        let playlist = fs::read_to_string(&path).unwrap();
        assert!(!playlist.contains("stream_0.ts"), "{playlist}");
        assert!(!dir.join("stream_0.ts").exists());
        for segment in playlist.lines().filter(|line| line.ends_with(".ts")) {
            assert!(
                dir.join(segment).exists(),
                "missing playlist segment {segment}"
            );
        }
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
            let bit_rate = parameters.bit_rate();
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
