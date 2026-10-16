//! Translate persisted output instances into independent engine destinations.
use ff_engine::{AsyncPlayout, OutputConfig};

use crate::utils::{
    config::{EncodingSource, OutputMode, PlayoutConfig},
    errors::ServiceError,
};

use super::playout::{
    engine_error, hls_playlist_path, open_desktop_playout, output_url, recording_config,
};

pub(super) async fn open_parallel_playout(
    config: &PlayoutConfig,
    program: &mut OutputConfig,
    attachments: &mut Vec<crate::moq::OutputAttachment>,
) -> Result<AsyncPlayout, ServiceError> {
    let mut destinations = Vec::new();
    let desktop = config
        .active_outputs()
        .find(|output| output.mode == OutputMode::Desktop);
    // Desktop owns the program's realtime clock when present.
    if let Some(desktop) = desktop {
        if config.program.is_none() {
            program.width = desktop.width;
            program.height = desktop.height;
        }
        program.desktop_window_size = Some(if desktop.inherit_resolution {
            (program.width, program.height)
        } else {
            (desktop.width, desktop.height)
        });
        program.desktop_fullscreen = desktop.desktop_fullscreen;
    }
    let mut dedicated_recording_assigned = false;

    for output in config.active_outputs() {
        if output.mode == OutputMode::Desktop {
            continue;
        }
        let effective = config
            .effective_output(output)
            .map_err(ServiceError::Conflict)?;
        let output = &effective;
        let mut scoped = config.clone();
        scoped.output = output.clone();
        if output.mode == OutputMode::HLS && output.id != config.output.id {
            scoped.channel.public = scoped.channel.public.join(format!("outputs/{}", output.id));
        }
        scoped.outputs.clear();
        let record_here = config.recording.enable
            && match config.recording.source {
                EncodingSource::Encode => {
                    let assign = desktop.is_none() && !dedicated_recording_assigned;
                    dedicated_recording_assigned |= assign;
                    assign
                }
                _ => config.recording.source_output_id == Some(output.id),
            };
        scoped.recording.enable = record_here;
        let mut settings = program.clone();
        settings.destinations.clear();
        settings.encoding_source = if output.encoding_source_id.is_some() {
            let (owner, rendition) = config
                .encoding_source(output)
                .map_err(ServiceError::Conflict)?;
            Some((owner.id, rendition))
        } else {
            None
        };
        if !output.inherit_resolution {
            settings.width = output.width;
            settings.height = output.height;
        }
        settings.video_codec = output.video_codec.clone();
        settings.video_options = output.video_options.clone();
        settings.audio_codec = output.audio_codec.clone();
        settings.audio_options = output.audio_options.clone();
        settings.audio_bitrate = u64::from(output.audio_bitrate) * 1000;
        settings.stream_type = output.stream_type.engine_stream_type();
        settings.stream_format = output.stream_format.clone();
        settings.protocol_options = output.protocol_options.clone();
        settings.muxer_options = output.muxer_options.clone();
        settings.metadata_options = output.metadata_options.clone();
        settings.recording = recording_config(&scoped)?;
        let target = match output.mode {
            OutputMode::HLS => {
                let variants = output.hls_streams().map_err(ServiceError::Conflict)?;
                let subtitle = config
                    .processing
                    .hls_subtitle()
                    .map_err(ServiceError::Conflict)?;
                let variants = if subtitle.is_some() || variants.len() > 1 {
                    variants
                } else {
                    Vec::new()
                };

                ff_engine::OutputTarget::Hls {
                    playlist: hls_playlist_path(&scoped)?.to_string_lossy().into_owned(),
                    variants,
                    subtitle,
                    segment_seconds: output.hls_segment_duration,
                    list_size: output.hls_list_size,
                }
            }
            OutputMode::Stream => ff_engine::OutputTarget::Stream {
                url: output_url(&scoped)?,
            },
            OutputMode::Moq => {
                let attachment = crate::moq::OutputAttachment::new(config.general.channel_id);
                settings.stream_endpoint = Some(attachment.endpoint());
                attachments.push(attachment);
                settings.stream_type = ff_engine::StreamType::Custom;
                settings.stream_format = "mpegts".into();
                ff_engine::OutputTarget::Stream { url: String::new() }
            }
            OutputMode::Desktop => unreachable!(),
        };
        destinations.push(ff_engine::OutputDestination {
            id: output.id,
            config: settings,
            target,
        });
    }
    program.destinations = destinations;
    program.recording = if desktop.is_some() && config.recording.source == EncodingSource::Encode {
        recording_config(config)?
    } else {
        None
    };
    if desktop.is_some() {
        open_desktop_playout(program.clone(), 10.0).await
    } else {
        AsyncPlayout::open_parallel(program.clone(), 10.0)
            .await
            .map_err(engine_error)
    }
}
