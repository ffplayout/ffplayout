use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use ffmpeg_next::{ChannelLayout, Packet, Rational, Rescale, codec, frame, software::resampling};

use crate::{
    OutputConfig, benchmark::Stage, playout::DecodeRecovery,
    utils::config::engine_audio_sample_format,
};

use super::{
    super::encoding::{
        AudioOutputStream, VideoOutputStream,
        audio::{AudioFrameBuffer, open_audio_encoder},
        video::{open_video_encoder, stream_gop_size},
    },
    ForwardMode, Track,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Profile {
    pub(super) mode: ForwardMode,
    pub(super) resize: bool,
    pub(super) dimensions: (u32, u32),
    fps: u32,
    sample_rate: u32,
    video: String,
    video_options: BTreeMap<String, String>,
    audio: String,
    audio_options: BTreeMap<String, String>,
    bitrate: u64,
    pub(super) channel_id: i32,
}

impl Profile {
    pub(super) fn new(cfg: &OutputConfig) -> Self {
        Self {
            mode: cfg.forward_mode,
            resize: cfg.forward_resize,
            dimensions: if cfg.forward_resize {
                (cfg.width, cfg.height)
            } else {
                (0, 0)
            },
            fps: cfg.fps,
            sample_rate: cfg.sample_rate,
            video: cfg.video_codec.clone(),
            video_options: cfg.video_options.clone(),
            audio: cfg.audio_codec.clone(),
            audio_options: cfg.audio_options.clone(),
            bitrate: cfg.audio_bitrate,
            channel_id: cfg.channel_id.unwrap_or_default(),
        }
    }

    fn config(&self, width: u32, height: u32) -> OutputConfig {
        let (width, height) = if self.resize {
            self.dimensions
        } else {
            (width, height)
        };
        let mut cfg = OutputConfig::new(width, height, self.fps, self.sample_rate);
        cfg.video_codec = self.video.clone();
        cfg.video_options = self.video_options.clone();
        cfg.audio_codec = self.audio.clone();
        cfg.audio_options = self.audio_options.clone();
        cfg.audio_bitrate = self.bitrate;

        cfg
    }
}

type Encoded = Vec<(Packet, Rational)>;

#[derive(Debug)]
pub(super) struct DecodeFailure;

impl std::fmt::Display for DecodeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("decoding forwarding packet")
    }
}

pub(super) struct TrackEncoder {
    time_base: Rational,
    codec: TrackCodec,
    recovery: DecodeRecovery,
}

enum TrackCodec {
    Video {
        decoder: codec::decoder::Video,
        encoder: VideoOutputStream,
        last_pts: Option<i64>,
    },
    Audio {
        decoder: codec::decoder::Audio,
        encoder: AudioOutputStream,
        resampler: Option<resampling::Context>,
        buffer: AudioFrameBuffer,
        next_pts: Option<i64>,
    },
}

impl TrackEncoder {
    pub(super) fn new(
        track: &Track,
        profile: &Profile,
        global_header: bool,
        index: usize,
    ) -> Result<Self> {
        let mut context =
            codec::context::Context::from_parameters(track.parameters.clone())?.decoder();

        context.set_packet_time_base(track.time_base);

        let codec = if track.video {
            let decoder = context
                .video()
                .context("opening forwarding video decoder")?;

            ensure!(
                decoder.width() > 0 && decoder.height() > 0,
                "missing forwarding video dimensions"
            );
            let cfg = profile.config(decoder.width(), decoder.height());
            let encoder = open_video_encoder(
                index,
                &cfg,
                global_header,
                None,
                (decoder.width(), decoder.height()),
                Some(stream_gop_size(cfg.fps)),
                true,
            )
            .context("opening forwarding video encoder")?;

            TrackCodec::Video {
                decoder,
                encoder,
                last_pts: None,
            }
        } else {
            let decoder = context
                .audio()
                .context("opening forwarding audio decoder")?;
            let cfg = profile.config(0, 0);
            let encoder = open_audio_encoder(index, &cfg, global_header, None)
                .context("opening forwarding audio encoder")?;
            let buffer =
                AudioFrameBuffer::new(cfg.sample_rate, encoder.encoder.frame_size() as usize);

            TrackCodec::Audio {
                decoder,
                encoder,
                resampler: None,
                buffer,
                next_pts: None,
            }
        };

        Ok(Self {
            time_base: track.time_base,
            codec,
            recovery: if track.video {
                DecodeRecovery::new(Stage::VideoDecode, "forwarding video", profile.channel_id)
            } else {
                DecodeRecovery::new(Stage::AudioDecode, "forwarding audio", profile.channel_id)
            },
        })
    }

    pub(super) fn parameters(&self) -> (codec::Parameters, Rational) {
        match &self.codec {
            TrackCodec::Video { encoder, .. } => (
                codec::Parameters::from(&encoder.encoder),
                encoder.encoder.time_base(),
            ),
            TrackCodec::Audio { encoder, .. } => (
                codec::Parameters::from(&encoder.encoder),
                encoder.encoder.time_base(),
            ),
        }
    }

    pub(super) fn submit(&mut self, packet: &Packet) -> Result<Encoded> {
        let mut packets = Vec::new();
        let mut emit = |packet, _, time_base| {
            packets.push((packet, time_base));

            Ok(())
        };

        match &mut self.codec {
            TrackCodec::Video {
                decoder,
                encoder,
                last_pts,
            } => {
                self.recovery.send(decoder, packet).context(DecodeFailure)?;
                let mut frame = frame::Video::empty();

                while self
                    .recovery
                    .receive(decoder, &mut frame)
                    .context(DecodeFailure)?
                {
                    let Some(pts) = frame.timestamp().or_else(|| frame.pts()) else {
                        continue;
                    };
                    let pts = pts.rescale(self.time_base, encoder.encoder.time_base());

                    if last_pts.is_some_and(|last| pts <= last) {
                        continue;
                    }

                    *last_pts = Some(pts);
                    frame.set_pts(Some(pts));
                    frame.set_kind(ffmpeg_next::picture::Type::None);
                    encoder.submit(&frame)?;
                    encoder.drain(&mut emit)?;
                }
            }
            TrackCodec::Audio {
                decoder,
                encoder,
                resampler,
                buffer,
                next_pts,
            } => {
                self.recovery.send(decoder, packet).context(DecodeFailure)?;
                let mut frame = frame::Audio::empty();

                while self
                    .recovery
                    .receive(decoder, &mut frame)
                    .context(DecodeFailure)?
                {
                    let rate = encoder.encoder.rate();
                    let layout = if frame.channel_layout().is_empty() {
                        ChannelLayout::default(i32::from(frame.channels()))
                    } else {
                        frame.channel_layout()
                    };

                    if resampler.as_ref().is_none_or(|context| {
                        let input = context.input();
                        input.format != frame.format()
                            || input.channel_layout != layout
                            || input.rate != frame.rate()
                    }) {
                        *resampler = Some(resampling::Context::get(
                            frame.format(),
                            layout,
                            frame.rate(),
                            engine_audio_sample_format(),
                            ChannelLayout::STEREO,
                            rate,
                        )?);
                        *next_pts = None;
                    }

                    frame.set_channel_layout(layout);
                    let timestamp = frame
                        .timestamp()
                        .or_else(|| frame.pts())
                        .map(|pts| pts.rescale(self.time_base, Rational(1, rate as i32)));
                    let pts = match (timestamp, *next_pts) {
                        (Some(a), Some(b)) if a.abs_diff(b) <= u64::from(rate / 10) => Some(b),
                        (Some(a), _) => Some(a),
                        (None, b) => b,
                    };
                    let context = resampler.as_mut().context("missing forwarding resampler")?;
                    let capacity = frame
                        .samples()
                        .saturating_mul(rate as usize)
                        .div_ceil(frame.rate().max(1) as usize)
                        .saturating_add(context.delay().map_or(0, |delay| delay.output as usize))
                        .saturating_add(1);
                    let mut converted = frame::Audio::new(
                        engine_audio_sample_format(),
                        capacity,
                        ChannelLayout::STEREO,
                    );

                    context.run(&frame, &mut converted)?;

                    if converted.samples() == 0 {
                        continue;
                    }

                    converted.set_rate(rate);
                    converted.set_pts(pts);
                    *next_pts = pts.map(|pts| pts.saturating_add(converted.samples() as i64));

                    for frame in buffer.push(&converted)? {
                        encoder.submit(&frame)?;
                        encoder.drain(&mut emit)?;
                    }
                }
            }
        }

        Ok(packets)
    }
}
