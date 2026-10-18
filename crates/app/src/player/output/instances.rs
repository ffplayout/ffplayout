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
    moq_demanded: bool,
) -> Result<AsyncPlayout, ServiceError> {
    configure_destinations(config, program, attachments, config.output.id, moq_demanded)?;
    if config
        .active_outputs()
        .any(|output| output.mode == OutputMode::Desktop)
    {
        open_desktop_playout(program.clone(), 10.0).await
    } else {
        AsyncPlayout::open_parallel(program.clone(), 10.0)
            .await
            .map_err(engine_error)
    }
}

pub(super) fn configure_destinations(
    config: &PlayoutConfig,
    program: &mut OutputConfig,
    attachments: &mut Vec<crate::moq::OutputAttachment>,
    recording_owner: i32,
    moq_demanded: bool,
) -> Result<(), ServiceError> {
    let mut destinations = Vec::new();
    let desktop = config
        .active_outputs()
        .find(|output| output.mode == OutputMode::Desktop);
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
    let mut required = std::collections::BTreeSet::new();
    let deliver = |output: &crate::utils::config::Output| {
        output.enabled && (output.mode != OutputMode::Moq || output.public_moq || moq_demanded)
    };
    for output in config.active_outputs().filter(|output| deliver(output)) {
        required.insert(output.id);
        if output.encoding_source_id.is_some() {
            required.insert(
                config
                    .encoding_source(output)
                    .map_err(ServiceError::Conflict)?
                    .0
                    .id,
            );
        }
    }
    if config.recording.enable
        && config.recording.source != EncodingSource::Encode
        && let Some(source) = config.output_source(config.recording.source_output_id)
    {
        required.insert(source.id);
        if source.encoding_source_id.is_some() {
            required.insert(
                config
                    .encoding_source(source)
                    .map_err(ServiceError::Conflict)?
                    .0
                    .id,
            );
        }
    }
    if config.recording.enable
        && config.recording.source == EncodingSource::Encode
        && desktop.is_none()
    {
        let source = config.output_source(Some(recording_owner)).ok_or_else(|| {
            ServiceError::Conflict(
                "recording encoder owner was removed; restart the channel".into(),
            )
        })?;
        required.insert(source.id);
        if source.encoding_source_id.is_some() {
            required.insert(
                config
                    .encoding_source(source)
                    .map_err(ServiceError::Conflict)?
                    .0
                    .id,
            );
        }
    }

    for output in std::iter::once(&config.output)
        .chain(config.outputs.iter())
        .filter(|output| required.contains(&output.id))
    {
        if output.mode == OutputMode::Desktop {
            continue;
        }
        let effective = config
            .effective_output(output)
            .map_err(ServiceError::Conflict)?;
        let output = &effective;
        let mut scoped = config.clone();
        scoped.output = output.clone();
        scoped.outputs.clear();
        let record_here = config.recording.enable
            && match config.recording.source {
                EncodingSource::Encode => desktop.is_none() && output.id == recording_owner,
                _ => config.recording.source_output_id == Some(output.id),
            };
        scoped.recording.enable = record_here;
        let mut settings = program.clone();
        settings.delivery_enabled = deliver(output);
        settings.destination_revision = destination_revision(output)?;
        let (owner, _) = config
            .encoding_source(output)
            .map_err(ServiceError::Conflict)?;
        settings.encoding_revision = encoding_revision(owner, config);
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
    Ok(())
}

fn destination_revision(output: &crate::utils::config::Output) -> Result<String, ServiceError> {
    let mut value = serde_json::to_value(output)
        .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
    if let Some(value) = value.as_object_mut() {
        for key in ["enabled", "label", "public_moq", "share_encoding"] {
            value.remove(key);
        }
    }

    Ok(value.to_string())
}

fn encoding_revision(output: &crate::utils::config::Output, config: &PlayoutConfig) -> String {
    let value = serde_json::json!({
        "width": output.width, "height": output.height, "inherit_resolution": output.inherit_resolution,
        "fps": output.fps, "video_codec": output.video_codec, "video_options": output.video_options,
        "audio_codec": output.audio_codec, "audio_options": output.audio_options, "audio_bitrate": output.audio_bitrate,
        "hls": (output.mode == OutputMode::HLS).then_some((&output.hls_variants, output.hls_segment_duration)),
        "recording": config.recording,
    });

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::config::Output;

    #[tokio::test]
    async fn private_moq_sleeps_without_demand_but_public_moq_always_delivers() {
        let moq = Output {
            id: 2,
            mode: OutputMode::Moq,
            enabled: true,
            ..Default::default()
        };
        let mut config = PlayoutConfig {
            output: moq,
            ..Default::default()
        };
        let mut program = OutputConfig::new(160, 90, 25, 48_000);
        let mut attachments = Vec::new();
        configure_destinations(&config, &mut program, &mut attachments, 2, false).unwrap();
        assert!(program.destinations.is_empty());
        assert!(attachments.is_empty());
        configure_destinations(&config, &mut program, &mut attachments, 2, true).unwrap();
        assert_eq!(program.destinations.len(), 1);
        assert!(program.destinations[0].config.delivery_enabled);
        config.output.public_moq = true;
        configure_destinations(&config, &mut program, &mut attachments, 2, false).unwrap();
        assert!(program.destinations[0].config.delivery_enabled);
        config.output.enabled = false;
        configure_destinations(&config, &mut program, &mut attachments, 2, true).unwrap();
        assert!(program.destinations.is_empty());
    }

    #[tokio::test]
    async fn sleeping_moq_keeps_encoding_for_recording_and_other_outputs() {
        let directory = std::env::temp_dir().join(format!("demand-plan-{}", uuid::Uuid::new_v4()));
        let moq = Output {
            id: 1,
            mode: OutputMode::Moq,
            enabled: true,
            ..Default::default()
        };
        let stream = Output {
            id: 2,
            mode: OutputMode::Stream,
            enabled: true,
            encoding_source_id: Some(1),
            stream_url: "rtmp://example.invalid/live/test".into(),
            ..Default::default()
        };
        let mut config = PlayoutConfig {
            output: moq,
            outputs: vec![stream],
            ..Default::default()
        };
        let mut program = OutputConfig::new(160, 90, 25, 48_000);
        let mut attachments = Vec::new();
        configure_destinations(&config, &mut program, &mut attachments, 1, false).unwrap();
        assert_eq!(program.destinations.len(), 2);
        assert!(!program.destinations[0].config.delivery_enabled);
        assert!(program.destinations[1].config.delivery_enabled);
        assert_eq!(program.destinations[1].config.encoding_source, Some((1, 0)));
        config.outputs.clear();
        config.recording.enable = true;
        config.recording.source = EncodingSource::Stream;
        config.recording.source_output_id = Some(1);
        config.recording.path = directory.to_string_lossy().into_owned();
        config.recording.segment_duration = 60;
        configure_destinations(&config, &mut program, &mut attachments, 1, false).unwrap();
        assert_eq!(program.destinations.len(), 1);
        assert!(!program.destinations[0].config.delivery_enabled);
        assert!(program.destinations[0].config.recording.is_some());
        if directory.exists() {
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[tokio::test]
    async fn disabled_hls_owner_supplies_moq_until_its_last_consumer_is_disabled() {
        let hls = Output {
            id: 1,
            mode: OutputMode::HLS,
            enabled: false,
            hls_playlist_name: "base".into(),
            hls_variants: vec!["small:320x180:600k:96k".into()],
            ..Default::default()
        };
        let mut moq = hls.clone();
        moq.id = 2;
        moq.mode = OutputMode::Moq;
        moq.enabled = true;
        moq.encoding_source_id = Some(1);
        moq.encoding_variant = "small".into();
        let mut config = PlayoutConfig {
            output: hls,
            outputs: vec![moq],
            ..Default::default()
        };
        let directory = std::env::temp_dir().join(format!("output-plan-{}", uuid::Uuid::new_v4()));
        config.channel.public = directory.clone();
        let mut program = OutputConfig::new(1280, 720, 25, 48_000);
        let mut attachments = Vec::new();
        configure_destinations(&config, &mut program, &mut attachments, 1, true).unwrap();
        assert_eq!(program.destinations.len(), 2);
        assert!(!program.destinations[0].config.delivery_enabled);
        assert!(program.destinations[1].config.delivery_enabled);
        assert_eq!(program.destinations[1].config.encoding_source, Some((1, 1)));
        assert_eq!(
            program.destinations[0].config.encoding_revision,
            program.destinations[1].config.encoding_revision
        );
        config.outputs[0].enabled = false;
        configure_destinations(&config, &mut program, &mut attachments, 1, true).unwrap();
        assert!(program.destinations.is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
