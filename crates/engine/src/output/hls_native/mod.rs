//! FFmpeg adapter for the codec-independent ff-hls packager.
use anyhow::{Context, Result, ensure};
use ff_hls::{Config, Cue, MediaPacket, Packager, RenditionConfig, Subtitle};
use ffmpeg_next::{Packet, Rational, Rescale, format, rescale};

mod codecs;
mod container;
mod fmp4;

use crate::utils::{
    config::{HlsSubtitle, HlsVariant, OutputConfig},
    ffmpeg::reference_packet,
};

use codecs::Codecs;
use container::{ContainerWriter, Track};

pub(super) struct NativeHls {
    packager: Packager<Packet, ContainerWriter>,
    time_bases: Vec<Rational>,
    renditions: usize,
    codecs: Vec<Codecs>,
}

impl NativeHls {
    pub fn new(
        template: &format::context::Output,
        path: &str,
        config: &OutputConfig,
        variants: &[HlsVariant],
        subtitle: Option<&HlsSubtitle>,
        (segment_seconds, list_size): (u32, u32),
        replacement: Option<super::hls::HlsReplacement>,
    ) -> Result<Self> {
        ensure!(
            !path.contains("://"),
            "native HLS currently supports local files only"
        );

        ensure!(
            config.metadata_options.is_empty(),
            "native HLS does not yet support custom container metadata"
        );

        let path = std::path::Path::new(path);
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .context("HLS output name missing")?;
        let mut renditions = Vec::new();
        let mut tracks = Vec::new();
        let mut codecs = Vec::new();
        let mut fragmented = None;

        for index in 0..variants.len().max(1) {
            let video = template
                .stream(index * 2)
                .context("native HLS video stream missing")?;
            let audio = template
                .stream(index * 2 + 1)
                .context("native HLS audio stream missing")?;
            let video_parameters = video.parameters();
            let audio_parameters = audio.parameters();

            let rendition_fragmented = crate::native_hls_uses_fmp4(
                video_parameters.id().name(),
                audio_parameters.id().name(),
                config
                    .muxer_options
                    .get("hls_segment_type")
                    .map(String::as_str),
            )
            .map_err(anyhow::Error::msg)?;
            ensure!(
                fragmented.is_none_or(|value| value == rendition_fragmented),
                "native HLS renditions must use the same segment container"
            );
            fragmented = Some(rendition_fragmented);

            // SAFETY: the live template owns both codec parameter allocations.
            let (width, height, video_rate, audio_rate) = unsafe {
                let parameters = &*video_parameters.as_ptr();

                (
                    parameters.width as u32,
                    parameters.height as u32,
                    parameters.bit_rate.max(0) as u64,
                    (*audio_parameters.as_ptr()).bit_rate.max(0) as u64,
                )
            };
            let variant = variants.get(index);
            let bandwidth = variant.map_or(video_rate.max(config.video_maxrate()), |variant| {
                variant.video_bitrate
            }) + variant.map_or(audio_rate.max(config.audio_bitrate), |variant| {
                variant.audio_bitrate
            });

            let declaration =
                (!rendition_fragmented).then(|| Codecs::new(&video_parameters, &audio_parameters));

            renditions.push(RenditionConfig {
                name: variant
                    .map_or(stem, |variant| variant.name.as_str())
                    .to_owned(),
                width,
                height,
                bandwidth: bandwidth.max(128_000) * 12 / 10,
                codecs: declaration
                    .as_ref()
                    .and_then(Codecs::value)
                    .unwrap_or_default(),
            });
            codecs.extend(declaration);
            tracks.push([
                Track {
                    parameters: video_parameters.clone(),
                    time_base: video.time_base(),
                },
                Track {
                    parameters: audio_parameters.clone(),
                    time_base: audio.time_base(),
                },
            ]);
        }

        let count = renditions.len();
        let config = Config {
            directory: parent.to_owned(),
            shared_audio: config
                .muxer_options
                .get("hls_shared_audio")
                .is_some_and(|value| value == "1"),
            renditions,
            subtitle: subtitle.map(|subtitle| Subtitle {
                name: subtitle.name.clone(),
                language: subtitle.language.clone(),
                default: subtitle.default,
            }),
            master_playlist: (!variants.is_empty() || subtitle.is_some())
                .then(|| "master.m3u8".to_owned()),
            segment_seconds,
            video_fps: config.fps,
            list_size,
            delete_threshold: config
                .muxer_options
                .get("hls_delete_threshold")
                .map(|value| value.parse::<u32>())
                .transpose()
                .context("invalid native HLS delete threshold")?
                .unwrap_or(1),
            program_date_time: config
                .muxer_options
                .get("hls_flags")
                .is_some_and(|flags| flags.split('+').any(|flag| flag == "program_date_time")),
        };
        let counters = replacement
            .as_ref()
            .map(|previous| (previous.next_sequence, previous.discontinuity_sequence));
        let writer = ContainerWriter::new(
            tracks,
            fragmented.unwrap_or_default(),
            config.shared_audio,
            replacement,
        );
        let packager = if let Some((next, discontinuities)) = counters {
            Packager::new_replacing(config, writer, next, discontinuities)?
        } else {
            Packager::new(config, writer)?
        };

        Ok(Self {
            packager,
            time_bases: template
                .streams()
                .map(|stream| stream.time_base())
                .collect(),
            renditions: count,
            codecs,
        })
    }

    pub fn write(&mut self, packet: &Packet) -> Result<()> {
        let stream = packet.stream();
        let time_base = *self
            .time_bases
            .get(stream)
            .context("native HLS stream missing")?;
        let pts = packet.pts().context("native HLS requires packet PTS")?;
        let pts_us = pts.rescale(time_base, rescale::TIME_BASE);
        let duration_us = packet.duration().rescale(time_base, rescale::TIME_BASE);

        if stream >= self.renditions * 2 {
            return self.packager.write_cue(Cue {
                start_ms: pts_us / 1000,
                end_ms: (pts_us + duration_us) / 1000,
                text: std::str::from_utf8(packet.data().context("subtitle payload missing")?)?
                    .to_owned(),
            });
        }

        if let Some(codecs) = self
            .codecs
            .get_mut(stream / 2)
            .and_then(|codecs| codecs.observe(packet))
        {
            self.packager.set_codecs(stream / 2, codecs)?;
        }

        self.packager.write(MediaPacket {
            stream,
            pts_us,
            dts_us: packet
                .dts()
                .unwrap_or(pts)
                .rescale(time_base, rescale::TIME_BASE),
            duration_us,
            keyframe: packet.is_key(),
            bytes: packet.size(),
            payload: reference_packet(packet)?,
        })
    }

    pub fn finish(&mut self) -> Result<()> {
        self.packager.finish()
    }
}
