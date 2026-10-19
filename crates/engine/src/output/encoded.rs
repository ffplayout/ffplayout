use std::time::{Duration, Instant};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt, fs,
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use ffmpeg::{Packet, codec, format, frame, util::rational::Rational};
use ffmpeg_next as ffmpeg;

use crate::{
    HlsHealth,
    analysis::{audio_level::AudioLevelMeter, loudness::LoudnessMeter},
    audio_mixer::AudioEffectChain,
    benchmark::{self, Stage},
    utils::{
        config::{
            HlsSubtitle, HlsVariant, OutputConfig, validate_output_metadata,
            validate_output_protocol_options,
        },
        ffmpeg_capabilities::validate_muxer_options,
        helper::{is_network_url, open_network_output},
        pipeline_queue::PipelineActivity,
    },
};

use super::{
    hls,
    mux::MuxOutput,
    recording::{self, RecordingMonitor, RecordingMuxer},
    vtt,
};

use super::encoding::{
    AudioOutputStream, EncodeFrame, EncodedPacket, EncodingWorkers, VideoOutputStream,
};

pub(super) struct EncodedOutput {
    primary_timing: Option<super::diagnostics::PrimaryTiming>,
    video_time_base: Rational,
    mux: MuxOutput,
    encoding_workers: Option<EncodingWorkers>,
    video_stream_indices: Vec<usize>,
    audio_stream_indices: Vec<usize>,
    video_streams: Vec<VideoOutputStream>,
    audio_streams: Vec<AudioOutputStream>,
    subtitle_streams: Vec<SubtitleOutputStream>,
    vtt_subtitles: bool,
    pending_vtt_cues: VecDeque<vtt::VttCue>,
    audio_effects: AudioEffectChain,
    audio_level_meter: AudioLevelMeter,
    loudness_meter: LoudnessMeter,
    audio_buffer: super::encoding::audio::AudioFrameBuffer,
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
        recording_cfg.protocol_options.clear();
        recording_cfg.metadata_options.clear();
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
        recording_cfg.output_timing_diagnostics = false;
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
        Self::open_internal(path, cfg, output_format, hls_health, true, None)
    }

    pub(super) fn delivery_state(&self) -> (bool, Option<String>) {
        self.mux.delivery_state()
    }

    pub(super) fn observe_delivery(&self, activity: Arc<PipelineActivity>) {
        self.mux.observe_delivery(activity);
    }

    pub(super) fn open_packets(
        path: &str,
        cfg: &OutputConfig,
        output_format: EncodedFormat,
        parameters: Vec<(codec::Parameters, codec::Parameters)>,
    ) -> Result<Self> {
        let health = matches!(output_format, EncodedFormat::Hls { .. }).then(HlsHealth::new);

        Self::open_internal(path, cfg, output_format, health, true, Some(parameters))
    }

    fn open_internal(
        path: &str,
        cfg: &OutputConfig,
        output_format: EncodedFormat,
        hls_health: Option<HlsHealth>,
        paced: bool,
        parameters: Option<Vec<(codec::Parameters, codec::Parameters)>>,
    ) -> Result<Self> {
        validate_output_metadata(&cfg.metadata_options).map_err(anyhow::Error::msg)?;

        match &output_format {
            EncodedFormat::Hls { .. } => {
                validate_muxer_options("hls", &cfg.muxer_options).map_err(anyhow::Error::msg)?;
            }
            EncodedFormat::Stream { muxer } => {
                validate_muxer_options(muxer, &cfg.muxer_options).map_err(anyhow::Error::msg)?;
            }
            EncodedFormat::Auto | EncodedFormat::Recording { .. } => {}
        }

        match &output_format {
            EncodedFormat::Stream { .. } if is_network_url(path) => {
                validate_output_protocol_options(cfg.stream_type, path, &cfg.protocol_options)
                    .map_err(anyhow::Error::msg)?;
            }
            EncodedFormat::Auto if is_network_url(path) => {
                validate_output_protocol_options(
                    crate::StreamType::Custom,
                    path,
                    &cfg.protocol_options,
                )
                .map_err(anyhow::Error::msg)?;
            }
            _ if !cfg.protocol_options.is_empty() => {
                return Err(anyhow!(
                    "protocol options can only be used with network stream outputs"
                ));
            }
            _ => {}
        }

        // Packet delivery already owns an isolated destination thread. HLS
        // needs no pacing, so write there instead of adding another queue and
        // mux thread. Standalone HLS and paced network outputs retain theirs.
        let packet_hls =
            parameters.is_some() && matches!(&output_format, EncodedFormat::Hls { .. });
        let threaded_output =
            paced && !packet_hls && !matches!(&output_format, EncodedFormat::Recording { .. });
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
                open_network_output(path, Some(muxer), &cfg.protocol_options)?
            }
            EncodedFormat::Stream { ref muxer } => format::output_as(path, muxer)?,
            EncodedFormat::Recording { .. } => recording::segment_output_context(Path::new(path))?,
            EncodedFormat::Auto if path.starts_with("rtmp://") || path.starts_with("rtmps://") => {
                open_network_output(path, Some("flv"), &cfg.protocol_options)?
            }
            EncodedFormat::Auto if is_network_url(path) => {
                open_network_output(path, None, &cfg.protocol_options)?
            }
            EncodedFormat::Auto => format::output(path)?,
        };

        if vtt_subtitles {
            // WebVTT is sparse and can remain empty for an entire clip. FFmpeg's
            // default interleave window holds A/V for up to ten seconds while
            // waiting for another cue. Keep live HLS delivery bounded instead.
            // SAFETY: this output context is exclusively owned before its header
            // is written or it is handed to the mux worker.
            unsafe {
                (*octx.as_mut_ptr()).max_interleave_delta = 100_000;
            }
        }

        if !cfg.metadata_options.is_empty()
            && !matches!(output_format, EncodedFormat::Recording { .. })
        {
            let mut metadata = ffmpeg::Dictionary::new();

            for (key, value) in &cfg.metadata_options {
                metadata.set(key, value);
            }
            octx.set_metadata(metadata);
        }
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
            if let Some(parameters) = &parameters {
                anyhow::ensure!(
                    parameters.len() == stream_count,
                    "encoding rendition count must match the muxer"
                );
                let (video, audio) = &parameters[index];
                let mut stream = octx.add_stream(codec::Id::None)?;
                stream.set_parameters(video.clone());
                stream.set_time_base(cfg.video_time_base);
                let mut stream = octx.add_stream(codec::Id::None)?;
                stream.set_parameters(audio.clone());
                stream.set_time_base(cfg.audio_time_base);
                continue;
            }
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
                if let Some(parameters) = &parameters {
                    let (video, audio) = parameters
                        .get(recording_video_stream_index)
                        .context("recording source rendition is unavailable")?;
                    return RecordingMuxer::open_parameters(
                        recording_config,
                        (video.clone(), cfg.video_time_base),
                        (audio.clone(), cfg.audio_time_base),
                    );
                }
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

        let audio_buffer = super::encoding::audio::AudioFrameBuffer::new(
            cfg.sample_rate,
            audio_streams
                .first()
                .map_or(0, |stream| stream.encoder.frame_size() as usize),
        );
        Ok(Self {
            primary_timing: (cfg.output_timing_diagnostics && log::log_enabled!(log::Level::Trace))
                .then(|| {
                    super::diagnostics::PrimaryTiming::new(cfg.channel_id.unwrap_or_default())
                }),
            video_time_base: cfg.video_time_base,
            mux: MuxOutput::new(octx, threaded_output, hls_health, cfg.channel_id)?,
            encoding_workers: None,
            video_stream_indices: if parameters.is_some() {
                (0..stream_count).map(|index| index * 2).collect()
            } else {
                video_streams
                    .iter()
                    .map(|stream| stream.stream_index)
                    .collect()
            },
            audio_stream_indices: if parameters.is_some() {
                (0..stream_count).map(|index| index * 2 + 1).collect()
            } else {
                audio_streams
                    .iter()
                    .map(|stream| stream.stream_index)
                    .collect()
            },
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
            audio_buffer,
            recording,
            recording_video_stream_index,
            transcoded_recording,
            recording_monitor: None,
            channel_id: cfg.channel_id,
        })
    }

    pub(super) fn threaded(mut self) -> Result<Self> {
        if let Some(recording) = self.transcoded_recording.take() {
            self.transcoded_recording = Some(Box::new(recording.threaded()?));
        }
        self.encoding_workers = Some(EncodingWorkers::start(
            std::mem::take(&mut self.video_streams),
            std::mem::take(&mut self.audio_streams),
        )?);

        Ok(self)
    }

    fn submit_encoding(&mut self, frame: EncodeFrame) -> Result<()> {
        let mut workers = self
            .encoding_workers
            .take()
            .context("encoder workers missing")?;
        let result = workers.submit(frame, |packet| self.handle_encoded_packet(packet));
        self.encoding_workers = Some(workers);

        result
    }

    pub(super) fn handle_encoded_packet(&mut self, mut packet: EncodedPacket) -> Result<()> {
        let video = self
            .video_stream_indices
            .get(self.recording_video_stream_index)
            == Some(&packet.stream);
        let audio = self
            .audio_stream_indices
            .get(self.recording_video_stream_index)
            == Some(&packet.stream);
        if let Some(recording) = &mut self.recording {
            let result = if video {
                recording.write_video(&packet.packet, packet.time_base)
            } else if audio {
                recording.write_audio(&packet.packet, packet.time_base)
            } else {
                Ok(())
            };
            if let Err(error) = result {
                self.disable_copy_recording(error);
            }
        }

        self.write_packet(&mut packet.packet, packet.stream, packet.time_base)
    }

    pub(super) fn playout_clock(&self) -> Option<crate::clock::SharedPlayoutClock> {
        self.mux.clock()
    }

    pub(super) fn audio_frame_size(&self) -> usize {
        if let Some(workers) = &self.encoding_workers {
            return workers.audio_frame_size;
        }
        self.audio_streams
            .first()
            .map(|stream| stream.encoder.frame_size() as usize)
            .unwrap_or(0)
    }

    pub(super) fn set_playout_rate(&mut self, rate: f64) {
        self.mux.set_rate(rate);
    }

    pub(super) fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        let entered = self.primary_timing.as_mut().map(|timing| {
            timing.begin();
            Instant::now()
        });
        let (mut submit, mut drain) = (Duration::ZERO, Duration::ZERO);
        benchmark::measure(Stage::EncodeMux, || {
            if self.encoding_workers.is_some() {
                let submit_started = entered.map(|_| Instant::now());
                let result = self.submit_encoding(EncodingWorkers::video_frame(frame)?);
                if let Some(started) = submit_started {
                    submit += started.elapsed();
                }

                return result;
            }
            for index in 0..self.video_streams.len() {
                let submit_started = entered.map(|_| Instant::now());
                self.video_streams[index].submit(frame)?;
                let drain_started = submit_started.map(|started| {
                    let now = Instant::now();
                    submit += now.saturating_duration_since(started);
                    now
                });
                self.write_video_packets(index)?;
                if let Some(started) = drain_started {
                    drain += started.elapsed();
                }
            }
            Ok::<_, anyhow::Error>(())
        })?;

        if let (Some(timing), Some(entered)) = (&mut self.primary_timing, entered) {
            timing.finish(frame.pts(), self.video_time_base, entered, submit, drain);
        }

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
        self.encode_audio_with_effects(frame, true)
    }

    pub(super) fn encode_processed_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.encode_audio_with_effects(frame, false)
    }

    fn encode_audio_with_effects(
        &mut self,
        frame: &frame::Audio,
        apply_effects: bool,
    ) -> Result<()> {
        if frame.samples() == 0 {
            return Ok(());
        }

        let mut frame = frame.clone();
        let frames = benchmark::measure(Stage::AudioProcess, || {
            if apply_effects {
                self.audio_effects.process(&mut frame);
            }
            self.audio_level_meter.process_frame(&frame);
            self.loudness_meter.process_frame(&frame);
            self.audio_buffer.push(&frame)
        })?;
        for frame in frames {
            self.send_audio_frame(&frame)?;
        }

        if let Some(error) = self
            .transcoded_recording
            .as_mut()
            .and_then(|recording| recording.encode_audio(&frame).err())
        {
            self.disable_transcoded_recording(error);
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
                // A seek may land inside a cue. Its visible remainder starts at
                // the new output boundary, never before the program timeline.
                start_ms: output_start_ms + cue.start_ms.saturating_sub(source_start_ms).max(0),
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

    fn send_audio_frame(&mut self, frame: &frame::Audio) -> Result<()> {
        benchmark::measure(Stage::AudioEncode, || {
            if self.encoding_workers.is_some() {
                return self.submit_encoding(EncodingWorkers::audio_frame(frame)?);
            }
            for index in 0..self.audio_streams.len() {
                self.audio_streams[index].submit(frame)?;
                self.write_audio_packets(index)?;
            }
            Ok::<_, anyhow::Error>(())
        })
    }

    fn pad_audio_buffer(&mut self) -> Result<()> {
        for frame in self.audio_buffer.finish()? {
            self.send_audio_frame(&frame)?;
        }
        Ok(())
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

        let stream_time_base = self.mux.time_base(stream_index)?;

        packet.set_stream(stream_index);
        packet.rescale_ts(encoder_time_base, stream_time_base);

        let queued = self.primary_timing.as_ref().map(|_| Instant::now());
        self.mux.write(packet)?;
        if let (Some(timing), Some(queued)) = (&mut self.primary_timing, queued) {
            timing.packet(queued.elapsed());
        }

        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<()> {
        benchmark::measure(Stage::AudioEncode, || {
            self.pad_audio_buffer()?;
            if let Some(mut workers) = self.encoding_workers.take() {
                workers.finish(|packet| self.handle_encoded_packet(packet))?;
            }

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

            self.mux.finish()?;

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
        let mut packets = Vec::new();
        self.audio_streams[index].flush_resampler(|packet, stream, time_base| {
            packets.push(EncodedPacket {
                packet,
                stream,
                time_base,
            });
            Ok(())
        })?;
        for packet in packets {
            self.handle_encoded_packet(packet)?;
        }

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

    fn log_recording_error(&self, error: impl fmt::Display) {
        log::error!(channel = self.channel_id.unwrap_or_default(); "Recording disabled: {error}");
    }

    fn write_subtitle_packet(&mut self, packet: &mut Packet) -> Result<()> {
        let stream_index = self
            .subtitle_streams
            .first()
            .context("subtitle output stream is missing")?
            .stream_index;
        let stream_time_base = self.mux.time_base(stream_index)?;

        packet.set_stream(stream_index);
        packet.rescale_ts(Rational(1, 1_000), stream_time_base);
        self.mux.write(packet)?;

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

fn open_video_stream(
    octx: &mut format::context::Output,
    cfg: &OutputConfig,
    output_format: EncodedFormat,
    global_header: bool,
    variant: Option<&HlsVariant>,
) -> Result<VideoOutputStream> {
    let input_size = match &output_format {
        EncodedFormat::Recording {
            input_width,
            input_height,
            ..
        } => (*input_width, *input_height),
        _ => cfg.input_size.unwrap_or((cfg.width, cfg.height)),
    };
    let gop = match &output_format {
        EncodedFormat::Hls {
            segment_seconds, ..
        } => Some(super::encoding::video::hls_gop_size(
            cfg.fps,
            *segment_seconds,
        )),
        EncodedFormat::Stream { .. } | EncodedFormat::Recording { .. } => {
            Some(super::encoding::video::stream_gop_size(cfg.fps))
        }
        EncodedFormat::Auto => None,
    };
    let encoder = super::encoding::video::open_video_encoder(
        octx.nb_streams() as usize,
        cfg,
        global_header,
        variant,
        input_size,
        gop,
        matches!(output_format, EncodedFormat::Hls { .. }),
    )?;
    let mut stream = octx.add_stream(codec::Id::None)?;
    stream.set_parameters(&encoder.encoder);
    stream.set_time_base(cfg.video_time_base);
    Ok(encoder)
}
fn open_audio_stream(
    octx: &mut format::context::Output,
    cfg: &OutputConfig,
    global_header: bool,
    variant: Option<&HlsVariant>,
) -> Result<AudioOutputStream> {
    let encoder = super::encoding::audio::open_audio_encoder(
        octx.nb_streams() as usize,
        cfg,
        global_header,
        variant,
    )?;
    let mut stream = octx.add_stream(codec::Id::None)?;
    stream.set_parameters(&encoder.encoder);
    stream.set_time_base(cfg.audio_time_base);
    Ok(encoder)
}

#[cfg(test)]
mod open_tests {
    use std::{
        env, fs,
        process::{self, Command},
    };

    use crate::utils::{
        config::{HlsSubtitle, OutputConfig},
        ffmpeg_capabilities::ffmpeg_capabilities,
    };

    use super::*;
    use ffmpeg::util::format::pixel::Pixel;
    use ffmpeg::{ChannelLayout, format::Sample};

    #[test]
    fn processed_audio_bypasses_volume_but_regular_audio_still_applies_it() {
        ffmpeg::init().unwrap();
        let directory = env::temp_dir().join(format!("processed_audio_test_{}", process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("processed_audio.ts");
        let config = OutputConfig::new(320, 240, 25, 48000);
        config.audio_effects.set_volume(1.5).unwrap();
        let mut output = EncodedOutput::open(
            path.to_str().unwrap(),
            &config,
            EncodedFormat::Stream {
                muxer: "mpegts".to_string(),
            },
        )
        .unwrap();
        let mut audio = frame::Audio::new(
            Sample::F32(ffmpeg::format::sample::Type::Planar),
            32,
            ChannelLayout::STEREO,
        );
        audio.set_rate(48000);
        audio.set_pts(Some(0));
        audio.plane_mut::<f32>(0).fill(0.2);
        audio.plane_mut::<f32>(1).fill(0.2);
        output.encode_processed_audio(&audio).unwrap();
        audio.set_pts(Some(32));
        output.encode_audio(&audio).unwrap();
        assert_eq!(output.audio_buffer.samples[0].len(), 64);
        for (index, &sample) in output.audio_buffer.samples[0].iter().enumerate() {
            let expected = if index < 32 { 0.2 } else { 0.3 };
            assert!((sample - expected).abs() < 1e-6);
        }
        output.finish().unwrap();
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn mpegts_writes_dvb_service_metadata() {
        ffmpeg::init().ok();
        let path = env::temp_dir().join(format!("ffplayout_metadata_{}.ts", process::id()));
        let cfg = OutputConfig::new(320, 240, 25, 44_100).with_metadata_options(BTreeMap::from([
            ("service_name".to_string(), "Example Channel".to_string()),
            (
                "service_provider".to_string(),
                "Example Provider".to_string(),
            ),
        ]));
        let mut output = EncodedOutput::open(
            path.to_str().unwrap(),
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
            video.data_mut(1).fill(128);
            video.data_mut(2).fill(128);
            output.encode_video(&video).unwrap();
        }
        output.finish().unwrap();

        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_programs",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&path)
            .output()
            .expect("ffprobe is required for FFmpeg integration tests");
        fs::remove_file(&path).ok();
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let programs = String::from_utf8(probe.stdout).unwrap();
        assert!(
            programs.contains("TAG:service_name=Example Channel"),
            "{programs}"
        );
        assert!(
            programs.contains("TAG:service_provider=Example Provider"),
            "{programs}"
        );
    }

    #[test]
    fn matroska_writes_generic_container_metadata() {
        ffmpeg::init().ok();
        let path =
            env::temp_dir().join(format!("ffplayout_generic_metadata_{}.mkv", process::id()));
        let cfg = OutputConfig::new(320, 240, 25, 44_100).with_metadata_options(BTreeMap::from([
            ("title".to_string(), "Example Program".to_string()),
            ("copyright".to_string(), "Example Copyright".to_string()),
        ]));
        let mut output = EncodedOutput::open(
            path.to_str().unwrap(),
            &cfg,
            EncodedFormat::Stream {
                muxer: "matroska".to_string(),
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
        }
        output.finish().unwrap();

        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_format",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&path)
            .output()
            .expect("ffprobe is required for FFmpeg integration tests");
        fs::remove_file(&path).ok();
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let format = String::from_utf8(probe.stdout).unwrap();
        assert!(format.contains("TAG:title=Example Program"), "{format}");
        assert!(
            format.contains("TAG:COPYRIGHT=Example Copyright"),
            "{format}"
        );
    }

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
        let dir = env::temp_dir().join(format!("hls_program_date_time_test_{}", process::id()));
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
    fn vtt_only_master_playlist_uses_literal_playlist_name() {
        ffmpeg::init().ok();
        let dir = env::temp_dir().join(format!("hls_vtt_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("hls_vtt_live_resume_test_{}", process::id()));
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

        output
            .write_vtt_subtitles(media_path.to_str().unwrap(), 1_040_000, 1_500)
            .unwrap();
        let resumed = output.pending_vtt_cues.front().unwrap();
        assert_eq!(resumed.start_ms, 1_040_000);
        assert_eq!(resumed.end_ms, 1_040_500);

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
        let dir = env::temp_dir().join(format!("hls_standalone_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("hls_cbr_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("stream_mpegts_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("recording_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("recording_failure_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("hls_recording_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("recording_encode_test_{}", process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let recording_path = dir.join("recording");
        let recording = crate::RecordingConfig::new(recording_path.to_string_lossy(), 300)
            .with_encode(crate::EncodingConfig {
                width: 160,
                height: 120,
                video_codec: "libx264".to_string(),
                video_options: crate::video_option_defaults("libx264"),
                audio_codec: "aac".to_string(),
                audio_options: BTreeMap::new(),
                audio_bitrate: 96_000,
            });
        // A dedicated recording must not inherit the network output's AVIO options.
        let network_cfg = OutputConfig::new(320, 240, 25, 44_100).with_protocol_options(
            BTreeMap::from([("latency".to_string(), "2000000".to_string())]),
        );
        EncodedOutput::open_recording(&network_cfg, &recording)
            .expect("network options must not disable the recording")
            .finish()
            .unwrap();
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

        let dir = env::temp_dir().join(format!("hls_fdk_aac_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("hls_append_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("hls_delete_test_{}", process::id()));
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
        let dir = env::temp_dir().join(format!("hls_multiple_streams_test_{}", process::id()));
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

        for bit_rate in &output.mux.stream_bitrates {
            assert!(*bit_rate > 0, "stream has no bitrate");
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
