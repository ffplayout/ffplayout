use std::{collections::HashSet, path::Path, sync::Arc};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use protect_axum::authorities::AuthDetails;
use serde::Serialize;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{Output, Role},
    },
    file::norm_abs_path,
    utils::{
        config::{OutputMode, PlayoutConfig, get_config},
        errors::ServiceError,
    },
};

#[derive(Debug, Serialize)]
pub struct CodecOption {
    pub name: String,
    pub display_name: String,
    pub codec_id: String,
    pub hardware: bool,
    pub uses_bitrate: bool,
    pub settings: Vec<EncoderSetting>,
}

#[derive(Debug, Serialize)]
pub struct EncoderSetting {
    pub key: String,
    pub label: String,
    pub kind: &'static str,
    pub default: String,
    pub choices: Vec<EncoderSettingChoice>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub visible_when: Option<EncoderSettingVisibility>,
}

#[derive(Debug, Serialize)]
pub struct EncoderSettingChoice {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct EncoderSettingVisibility {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct OutputCodecOptions {
    pub video: Vec<CodecOption>,
    pub audio: Vec<CodecOption>,
}

#[derive(Debug, Serialize)]
pub struct PlayoutCodecOptions {
    pub moq: OutputCodecOptions,
    pub hls: OutputCodecOptions,
    pub rtmp: OutputCodecOptions,
    pub srt: OutputCodecOptions,
    pub udp: OutputCodecOptions,
    pub custom: OutputCodecOptions,
    pub recording: OutputCodecOptions,
}

#[derive(Debug, Serialize)]
pub struct PlayoutConfigUpdate {
    pub requires_restart: bool,
}

/// Audio settings, mail and listener idle timeouts use live runtime controls. The
/// engine creates input, compositor and output contexts when playout starts,
/// therefore every other configuration change requires a fresh instance.
fn requires_playout_restart(current: &PlayoutConfig, updated: &PlayoutConfig) -> bool {
    let Ok(mut current) = serde_json::to_value(current) else {
        return true;
    };
    let Ok(mut updated) = serde_json::to_value(updated) else {
        return true;
    };

    for config in [&mut current, &mut updated] {
        let Some(config) = config.as_object_mut() else {
            return true;
        };

        for key in ["output", "outputs"] {
            if key == "output" {
                if let Some(output) = config
                    .get_mut(key)
                    .and_then(serde_json::Value::as_object_mut)
                {
                    output.remove("public_moq");
                    // Legacy sharing flags are normalized on save. The explicit
                    // encoding source below already describes the runtime change.
                    output.remove("share_encoding");
                }
            } else if let Some(outputs) = config
                .get_mut(key)
                .and_then(serde_json::Value::as_array_mut)
            {
                for output in outputs {
                    if let Some(output) = output.as_object_mut() {
                        output.remove("public_moq");
                        output.remove("share_encoding");
                    }
                }
            }
        }
        config.remove("mail");
        config.remove("notification");
        if let Some(listeners) = config
            .get_mut("ingest")
            .and_then(|ingest| ingest.get_mut("listeners"))
            .and_then(serde_json::Value::as_array_mut)
        {
            for listener in listeners {
                if let Some(listener) = listener.as_object_mut() {
                    listener.remove("idle_timeout_ms");
                }
            }
        }

        if let Some(audio) = config
            .get_mut("audio")
            .and_then(serde_json::Value::as_object_mut)
        {
            let scope = audio.get("loudness_scope").cloned();
            audio.clear();
            audio.insert("loudness_scope".to_string(), scope.unwrap_or_default());
        }
    }

    current != updated
}

fn running_listener_port_in_use(
    configs: &[(i32, Arc<PlayoutConfig>)],
    channel_id: i32,
    backend: &str,
    port: u16,
) -> bool {
    configs.iter().any(|(running_channel_id, config)| {
        *running_channel_id != channel_id
            && config.ingest.listeners.iter().any(|listener| {
                listener.enabled
                    && listener.backend == backend
                    && listener
                        .listen_port()
                        .is_ok_and(|used_port| used_port == port)
            })
    })
}

fn codec_option(codec: &ff_engine::FfmpegCodec) -> CodecOption {
    CodecOption {
        name: codec.name.clone(),
        display_name: codec.display_name.clone(),
        codec_id: codec.codec_id.clone(),
        hardware: codec.hardware,
        uses_bitrate: match codec.media_type {
            ff_engine::FfmpegMediaType::Video => ff_engine::video_codec_uses_bitrate(&codec.name),
            ff_engine::FfmpegMediaType::Audio => ff_engine::audio_codec_uses_bitrate(&codec.name),
            ff_engine::FfmpegMediaType::Subtitle => false,
        },
        settings: match codec.media_type {
            ff_engine::FfmpegMediaType::Video => ff_engine::video_option_specs(&codec.name),
            ff_engine::FfmpegMediaType::Audio => &[],
            ff_engine::FfmpegMediaType::Subtitle => &[],
        }
        .iter()
        .map(|setting| EncoderSetting {
            key: setting.key.to_string(),
            label: setting.label.to_string(),
            kind: match setting.kind {
                ff_engine::VideoOptionKind::Select => "select",
                ff_engine::VideoOptionKind::Number => "number",
            },
            default: setting.default.to_string(),
            choices: setting
                .choices
                .iter()
                .map(|choice| EncoderSettingChoice {
                    value: choice.value.to_string(),
                    label: choice.label.to_string(),
                })
                .collect(),
            minimum: setting.minimum,
            maximum: setting.maximum,
            visible_when: setting
                .visible_when
                .map(|condition| EncoderSettingVisibility {
                    key: condition.key.to_string(),
                    value: condition.value.to_string(),
                }),
        })
        .collect(),
    }
}

fn output_codec_options(target: ff_engine::FfmpegOutputTarget) -> OutputCodecOptions {
    let capabilities = ff_engine::ffmpeg_capabilities();

    OutputCodecOptions {
        video: capabilities
            .video_codecs_for(target)
            .iter()
            .map(codec_option)
            .collect(),
        audio: capabilities
            .audio_codecs_for(target)
            .iter()
            .filter(|codec| !codec.hardware)
            .map(codec_option)
            .collect(),
    }
}

fn moq_output_codec_options() -> OutputCodecOptions {
    let mut options = output_codec_options(ff_engine::FfmpegOutputTarget::Srt);
    options
        .video
        .retain(|codec| crate::utils::moq_codecs::supports_video(&codec.codec_id));
    options
        .audio
        .retain(|codec| crate::utils::moq_codecs::supports_audio(&codec.codec_id));

    options
}

fn custom_output_codec_options() -> OutputCodecOptions {
    let capabilities = ff_engine::ffmpeg_capabilities();

    OutputCodecOptions {
        video: capabilities
            .usable_codecs(ff_engine::FfmpegMediaType::Video)
            .iter()
            .map(codec_option)
            .collect(),
        audio: capabilities
            .usable_codecs(ff_engine::FfmpegMediaType::Audio)
            .iter()
            .filter(|codec| !codec.hardware)
            .map(codec_option)
            .collect(),
    }
}

/// **Get Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/config/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
pub async fn get_playout_config(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<PlayoutConfig>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;

    let config = manager.config.read().await.clone();

    Ok(Json(config))
}

/// **Update Config**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/playout/config/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn update_playout_config(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<PlayoutConfig>,
) -> Result<Json<PlayoutConfigUpdate>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;
    let p = manager.channel.lock().await.storage.clone();
    let storage = Path::new(&p);
    let config = manager.config.read().await.clone();
    let config_id = config.general.id;

    let (_, _, logo) = norm_abs_path(storage, &data.processing.logo)?;
    let (_, _, filler) = norm_abs_path(storage, &data.storage.filler)?;

    data.processing.logo = logo;
    data.storage.filler = filler;
    if let Some(preset_id) = data.text.preset_id {
        handles::select_preset(&state.pool, id, preset_id)
            .await
            .map_err(|_| ServiceError::BadRequest("invalid text preset".to_string()))?;
    }
    data.processing
        .hls_subtitle()
        .map_err(ServiceError::BadRequest)?;
    let mut listener_ports = HashSet::new();
    {
        let listeners = &data.ingest.listeners;

        if listeners.len() > 8 {
            return Err(ServiceError::BadRequest(
                "at most eight live listeners are supported".to_string(),
            ));
        }
        let mut ids = HashSet::new();
        let known_ids: HashSet<i32> = config
            .ingest
            .listeners
            .iter()
            .map(|listener| listener.id)
            .collect();

        for listener in listeners {
            if listener.idle_timeout_ms == 0 {
                return Err(ServiceError::BadRequest(
                    "live idle timeout must be greater than zero".to_string(),
                ));
            }

            if !matches!(listener.backend.as_str(), "rtmp" | "srt")
                || !(0..=100).contains(&listener.priority)
            {
                return Err(ServiceError::BadRequest(
                    "invalid live listener backend or priority (expected 0–100)".to_string(),
                ));
            }
            if listener.id < 0
                || (listener.id != 0
                    && (!ids.insert(listener.id) || !known_ids.contains(&listener.id)))
            {
                return Err(ServiceError::BadRequest(
                    "duplicate or invalid live listener ID".to_string(),
                ));
            }
            if listener.name.chars().count() > 128 || listener.identifier.len() > 2048 {
                return Err(ServiceError::BadRequest(
                    "live listener name or URL is too long".to_string(),
                ));
            }
            ff_engine::validate_input_protocol_options(&listener.backend, &listener.options)
                .map_err(ServiceError::BadRequest)?;
            ff_engine::validate_live_demuxer_options(&listener.backend, &listener.demuxer_options)
                .map_err(ServiceError::BadRequest)?;
            if let Some(name) = listener
                .demuxer_options
                .keys()
                .find(|name| listener.options.contains_key(*name))
            {
                return Err(ServiceError::BadRequest(format!(
                    "live input option {name:?} is configured for both protocol and demuxer"
                )));
            }

            if !listener.enabled {
                continue;
            }

            let backend = if listener.backend == "srt" {
                ff_engine::LiveInputBackend::Srt
            } else {
                ff_engine::LiveInputBackend::Rtmp
            };

            if !ff_engine::live_protocol_available(backend) {
                return Err(ServiceError::BadRequest(format!(
                    "FFmpeg input protocol {:?} is unavailable in this build",
                    backend
                )));
            }

            let port = listener.listen_port().map_err(ServiceError::BadRequest)?;

            if !listener_ports.insert((listener.backend.clone(), port)) {
                return Err(ServiceError::BadRequest(format!(
                    "live listener port {port} is assigned to multiple listeners"
                )));
            }
        }
    }
    ff_engine::AudioEffectsControl::new(data.audio.volume)
        .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
    validate_loudness(&data.audio)?;
    if !data.output.enabled {
        let index = data
            .outputs
            .iter()
            .position(|output| output.enabled)
            .ok_or_else(|| ServiceError::BadRequest("enable at least one output".into()))?;
        if data.program.is_none() {
            data.program = Some(crate::utils::config::ProgramFormat {
                width: data.output.width,
                height: data.output.height,
                fps: data.output.fps,
            });
        }
        std::mem::swap(&mut data.output, &mut data.outputs[index]);
    }
    if let Some(program) = &data.program {
        for output in std::iter::once(&mut data.output).chain(data.outputs.iter_mut()) {
            output.fps = program.fps;
            if output.inherit_resolution {
                output.width = program.width;
                output.height = program.height;
            }
        }
    }
    data.validate_outputs().map_err(ServiceError::BadRequest)?;
    data.recording
        .validate()
        .map_err(ServiceError::BadRequest)?;
    if data.recording.enable
        && data.recording.source != crate::utils::config::EncodingSource::Encode
    {
        let source = data
            .output_source(data.recording.source_output_id)
            .ok_or_else(|| {
                ServiceError::BadRequest("recording source must reference an active output".into())
            })?;
        let expected = match data.recording.source {
            crate::utils::config::EncodingSource::Stream => OutputMode::Stream,
            crate::utils::config::EncodingSource::HlsVariant => OutputMode::HLS,
            crate::utils::config::EncodingSource::Encode => unreachable!(),
        };
        if source.mode != expected
            && !(expected == OutputMode::Stream && source.mode == OutputMode::Moq)
        {
            return Err(ServiceError::BadRequest(
                "recording source type does not match output".into(),
            ));
        }
        if expected == OutputMode::HLS
            && !data
                .effective_output(source)
                .map_err(ServiceError::BadRequest)?
                .hls_streams()
                .map_err(ServiceError::BadRequest)?
                .iter()
                .any(|variant| variant.name == data.recording.variant)
        {
            return Err(ServiceError::BadRequest(
                "unknown recording HLS variant".into(),
            ));
        }
    }

    // Reserve the SQLite writer before checking other channels. Concurrent
    // saves must not both observe the same listener port as available.
    let mut transaction = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let running_configs = {
        let controller = state.controller.read().await;

        controller
            .managers
            .iter()
            .filter_map(|manager| manager.running_config().map(|config| (manager.id, config)))
            .collect::<Vec<_>>()
    };

    for (backend, port) in listener_ports {
        if running_listener_port_in_use(&running_configs, id, &backend, port)
            || handles::live_listener_port_in_use_on(&mut transaction, id, &backend, port).await?
        {
            return Err(ServiceError::BadRequest(format!(
                "live listener port {port} is already assigned to another listener"
            )));
        }
    }

    handles::save_outputs_on(&mut transaction, config_id, &mut data).await?;
    handles::update_recording_on(&mut transaction, id, &data.recording).await?;
    handles::update_configuration_on(&mut transaction, config_id, data).await?;
    transaction.commit().await?;
    let new_config = get_config(&state.pool, id).await?;
    let mut queues = state.mail_queues.lock().await;

    for queue in queues.iter_mut() {
        let mut queue_lock = queue.lock().await;

        if queue_lock.id == id {
            if queue_lock.config.recipient != new_config.mail.recipient {
                queue_lock.clear_raw();
            }

            queue_lock.update(new_config.mail.clone());
            queue_lock.update_notification(new_config.notification.clone());
            break;
        }
    }

    let running_config = manager.running_config();
    let requires_restart =
        requires_playout_restart(running_config.as_deref().unwrap_or(&config), &new_config);
    manager
        .audio_effects
        .set_volume(new_config.audio.volume)
        .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
    manager.live_loudness.update(
        running_config
            .as_deref()
            .unwrap_or(&config)
            .audio
            .loudness_scope
            != "off",
        crate::player::controller::loudness_config(&new_config.audio),
    );
    manager.update_config(new_config).await?;

    Ok(Json(PlayoutConfigUpdate { requires_restart }))
}

fn validate_loudness(processing: &crate::utils::config::Audio) -> Result<(), ServiceError> {
    if !matches!(processing.loudness_scope.as_str(), "all" | "live" | "off")
        || !processing.compressor_ratio.is_finite()
        || !(1.0..=10.0).contains(&processing.compressor_ratio)
        || !processing.compressor_threshold_dbfs.is_finite()
        || !(-60.0..=0.0).contains(&processing.compressor_threshold_dbfs)
        || !processing.pause_threshold_dbfs.is_finite()
        || !(-90.0..=-20.0).contains(&processing.pause_threshold_dbfs)
    {
        return Err(ServiceError::BadRequest(
            "Invalid audio dynamics settings".to_string(),
        ));
    }

    let dynamics_ranges = [
        (processing.compressor_attack_ms, 0.1..=50.0),
        (processing.compressor_hold_ms, 0.0..=2000.0),
        (processing.compressor_release_ms, 10.0..=10000.0),
        (processing.compressor_strong_release_ms, 10.0..=10000.0),
        (processing.compressor_knee_db, 0.0..=24.0),
        (processing.pause_hold_ms, 0.0..=5000.0),
        (processing.pause_return_delay_ms, 0.0..=30000.0),
        (processing.loudness_output_max_correction_db, 0.0..=12.0),
        (processing.loudness_output_gain_up_db_per_second, 0.0..=5.0),
        (
            processing.loudness_output_gain_down_db_per_second,
            0.0..=5.0,
        ),
    ];
    if dynamics_ranges
        .iter()
        .any(|(value, range)| !value.is_finite() || !range.contains(value))
        || processing.pause_return_delay_ms < processing.pause_hold_ms
    {
        return Err(ServiceError::BadRequest(
            "Invalid audio timing or output correction settings".to_string(),
        ));
    }

    let values = [
        processing.loudness_target_lufs,
        processing.loudness_dead_band_lu,
        processing.loudness_max_gain_db,
        processing.loudness_max_attenuation_db,
        processing.loudness_gain_up_db_per_second,
        processing.loudness_gain_down_db_per_second,
        processing.loudness_silence_gate_lufs,
        processing.loudness_true_peak_ceiling_dbtp,
    ];
    if values.iter().any(|value| !value.is_finite())
        || processing.loudness_dead_band_lu < 0.0
        || processing.loudness_max_gain_db < 0.0
        || processing.loudness_max_attenuation_db > 0.0
        || processing.loudness_gain_up_db_per_second < 0.0
        || processing.loudness_gain_down_db_per_second < 0.0
        || processing.loudness_true_peak_ceiling_dbtp > 0.0
    {
        return Err(ServiceError::BadRequest(
            "invalid live loudness settings".to_string(),
        ));
    }
    Ok(())
}

/// **Get Output**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/output/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
pub async fn get_playout_outputs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<Output>>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    if let Ok(mut outputs) = handles::select_outputs(&state.pool, id).await {
        let controller = state.controller.read().await;
        if let Some(manager) = controller.get(id) {
            let states = manager.output_status.snapshot();
            let running = manager.running_config();
            for output in &mut outputs {
                output.runtime_retryable = manager.output_status.can_retry(output.id);
                if let Some(status) = states.get(&output.id) {
                    output.runtime_status = Some(status.phase.to_string());
                    output.runtime_error = status.error.clone();
                } else if running.as_ref().is_some_and(|config| {
                    config.active_outputs().any(|active| active.id == output.id)
                }) {
                    output.runtime_status = Some("running".into());
                } else {
                    output.runtime_status = Some("idle".into());
                }
                if output.name == "moq"
                    && running.as_ref().is_some_and(|config| {
                        config.active_outputs().any(|active| active.id == output.id)
                    })
                    && let Some(status) = crate::moq::status(id)
                {
                    // Encoder status alone cannot tell whether the MoQ importer is alive.
                    if matches!(
                        output.runtime_status.as_deref(),
                        Some("running" | "starting")
                    ) {
                        output.runtime_status = Some(status.phase.into());
                        output.runtime_error = status.error;
                    }
                }
            }
        }
        return Ok(Json(outputs));
    }

    Err(ServiceError::InternalServerError)
}

/// Reconnect one running destination without changing saved settings or restarting playout.
pub async fn retry_playout_output(
    State(state): State<AppState>,
    AxumPath((channel, output)): AxumPath<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<bool>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(channel)?;
    let controller = state.controller.read().await;
    let manager = controller
        .get(channel)
        .ok_or_else(|| ServiceError::Conflict("channel is not running".into()))?;

    if !manager.output_status.request_retry(output) {
        return Err(ServiceError::Conflict(
            "output has no failed destination to reconnect".into(),
        ));
    }

    Ok(Json(true))
}

pub async fn get_playout_codecs(
    AxumPath(id): AxumPath<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<PlayoutCodecOptions>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_channel_or_admin(id)?;

    Ok(Json(PlayoutCodecOptions {
        moq: moq_output_codec_options(),
        hls: output_codec_options(ff_engine::FfmpegOutputTarget::Hls),
        rtmp: output_codec_options(ff_engine::FfmpegOutputTarget::Rtmp),
        srt: output_codec_options(ff_engine::FfmpegOutputTarget::Srt),
        udp: output_codec_options(ff_engine::FfmpegOutputTarget::Udp),
        custom: custom_output_codec_options(),
        recording: output_codec_options(ff_engine::FfmpegOutputTarget::Matroska),
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{requires_playout_restart, running_listener_port_in_use};
    use crate::utils::config::{LiveInput, PlayoutConfig};

    #[test]
    fn public_moq_permission_changes_do_not_restart_encoding() {
        let current = PlayoutConfig::default();
        let mut updated = current.clone();
        updated.output.public_moq = true;
        assert!(!requires_playout_restart(&current, &updated));
        updated.output.width += 2;
        assert!(requires_playout_restart(&current, &updated));
    }

    #[test]
    fn normalizing_the_legacy_sharing_flag_does_not_require_a_restart() {
        let mut current = PlayoutConfig::default();
        current.output.share_encoding = true;
        current.output.encoding_source_id = None;
        let mut updated = current.clone();
        updated.output.share_encoding = false;
        assert!(!requires_playout_restart(&current, &updated));
        updated.output.encoding_source_id = Some(42);
        assert!(requires_playout_restart(&current, &updated));
    }

    #[test]
    fn moq_codec_options_follow_importer_support_without_an_encoder_name_whitelist() {
        let options = super::moq_output_codec_options();
        let expected =
            ff_engine::ffmpeg_capabilities().video_codecs_for(ff_engine::FfmpegOutputTarget::Srt);
        for codec in expected
            .iter()
            .filter(|codec| crate::utils::moq_codecs::supports_video(&codec.codec_id))
        {
            assert!(options.video.iter().any(|option| option.name == codec.name));
        }
        assert!(
            options
                .video
                .iter()
                .all(|codec| crate::utils::moq_codecs::supports_video(&codec.codec_id))
        );
        assert!(
            options
                .audio
                .iter()
                .all(|codec| crate::utils::moq_codecs::supports_audio(&codec.codec_id))
        );
    }

    #[test]
    fn changing_only_idle_timeouts_does_not_require_restart() {
        let mut current = PlayoutConfig::default();
        current.ingest.listeners.push(LiveInput {
            id: 1,
            backend: "rtmp".to_string(),
            ..LiveInput::default()
        });
        let mut updated = current.clone();
        updated.ingest.listeners[0].idle_timeout_ms = 5000;
        assert!(!requires_playout_restart(&current, &updated));
        updated.ingest.listeners[0].identifier = "rtmp://127.0.0.1:1940/live/new".to_string();
        assert!(requires_playout_restart(&current, &updated));
        let mut later = updated.clone();
        later.ingest.listeners[0].idle_timeout_ms = 3000;
        assert!(requires_playout_restart(&current, &later));
    }

    #[test]
    fn changing_live_listeners_requires_playout_restart() {
        let current = PlayoutConfig::default();
        let mut updated = current.clone();
        updated.ingest.listeners = vec![LiveInput {
            backend: "srt".to_string(),
            identifier: "srt://127.0.0.1:9000".to_string(),
            enabled: true,
            ..LiveInput::default()
        }];

        assert!(requires_playout_restart(&current, &updated));
    }

    #[test]
    fn later_volume_save_still_requires_restart_for_unapplied_listener_change() {
        let running = PlayoutConfig::default();
        let mut saved = running.clone();
        saved.ingest.listeners.push(LiveInput {
            backend: "srt".to_string(),
            identifier: "srt://127.0.0.1:9000".to_string(),
            enabled: true,
            ..LiveInput::default()
        });
        let mut second_save = saved.clone();
        second_save.audio.volume = 0.75;

        assert!(!requires_playout_restart(&saved, &second_save));
        assert!(requires_playout_restart(&running, &second_save));
    }

    #[test]
    fn running_listener_reserves_its_old_port_until_playout_stops() {
        let mut running = PlayoutConfig::default();
        running.ingest.listeners.push(LiveInput {
            enabled: true,
            backend: "rtmp".to_string(),
            identifier: "rtmp://127.0.0.1:1936/live/stream".to_string(),
            ..LiveInput::default()
        });
        let configs = vec![(1, Arc::new(running))];
        let mut saved = (*configs[0].1).clone();
        saved.ingest.listeners[0].identifier = "rtmp://127.0.0.1:1940/live/stream".to_string();

        assert!(!running_listener_port_in_use(
            &[(1, Arc::new(saved))],
            2,
            "rtmp",
            1936
        ));
        assert!(running_listener_port_in_use(&configs, 2, "rtmp", 1936));
        assert!(!running_listener_port_in_use(&configs, 1, "rtmp", 1936));
        assert!(!running_listener_port_in_use(&configs, 2, "srt", 1936));

        let mut stopped = (*configs[0].1).clone();
        stopped.ingest.listeners[0].enabled = false;
        assert!(!running_listener_port_in_use(
            &[(1, Arc::new(stopped))],
            2,
            "rtmp",
            1936
        ));
    }

    #[test]
    fn notification_and_volume_changes_do_not_require_restart() {
        let current = PlayoutConfig::default();
        let mut updated = current.clone();
        updated.mail.recipient = "ops@example.org".to_string();
        updated.notification.topic = "ffplayout-alerts".to_string();
        updated.audio.volume = 0.75;

        assert!(!requires_playout_restart(&current, &updated));
    }

    #[test]
    fn loudness_parameter_changes_do_not_require_restart() {
        let current = PlayoutConfig::default();
        let mut updated = current.clone();
        updated.audio.loudness_enable = true;
        updated.audio.loudness_target_lufs = -23.0;

        assert!(!requires_playout_restart(&current, &updated));
    }

    #[test]
    fn scope_changes_require_restart_but_dynamics_parameters_do_not() {
        let current = PlayoutConfig::default();
        let mut updated = current.clone();
        updated.audio.loudness_scope = "live".to_string();
        assert!(requires_playout_restart(&current, &updated));
        updated.audio.loudness_scope = current.audio.loudness_scope.clone();
        updated.audio.compressor_ratio = 4.0;
        updated.audio.compressor_attack_ms = 10.0;
        updated.audio.compressor_hold_ms = 200.0;
        updated.audio.compressor_release_ms = 2400.0;
        updated.audio.compressor_strong_release_ms = 1000.0;
        updated.audio.compressor_knee_db = 12.0;
        updated.audio.pause_hold_ms = 600.0;
        updated.audio.pause_return_delay_ms = 4000.0;
        updated.audio.loudness_output_max_correction_db = 6.0;
        updated.audio.loudness_output_gain_up_db_per_second = 0.2;
        updated.audio.loudness_output_gain_down_db_per_second = 0.5;

        updated.audio.compressor_threshold_dbfs = -30.0;
        updated.audio.pause_threshold_dbfs = -65.0;
        assert!(!requires_playout_restart(&current, &updated));
    }

    #[test]
    fn dynamics_validation_rejects_invalid_scope_and_detector_parameters() {
        let mut audio = crate::utils::config::Audio {
            loudness_scope: "all".to_string(),
            compressor_ratio: 3.0,
            compressor_attack_ms: 5.0,
            compressor_hold_ms: 100.0,
            compressor_release_ms: 1200.0,
            compressor_strong_release_ms: 500.0,
            compressor_knee_db: 6.0,
            pause_hold_ms: 300.0,
            pause_return_delay_ms: 2000.0,
            loudness_output_max_correction_db: 3.0,
            loudness_output_gain_up_db_per_second: 0.1,
            loudness_output_gain_down_db_per_second: 0.25,

            compressor_threshold_dbfs: -26.0,
            pause_threshold_dbfs: -55.0,
            ..Default::default()
        };
        assert!(super::validate_loudness(&audio).is_ok());
        audio.compressor_attack_ms = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_attack_ms = 51.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_attack_ms = 5.0;
        audio.compressor_hold_ms = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_hold_ms = 2001.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_hold_ms = 100.0;
        audio.compressor_release_ms = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_release_ms = 10001.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_release_ms = 1200.0;
        audio.compressor_strong_release_ms = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_strong_release_ms = 10001.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_strong_release_ms = 500.0;
        audio.compressor_knee_db = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_knee_db = 25.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_knee_db = 6.0;
        audio.pause_hold_ms = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.pause_hold_ms = 5001.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.pause_hold_ms = 300.0;
        audio.pause_return_delay_ms = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.pause_return_delay_ms = 30001.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.pause_return_delay_ms = 2000.0;
        audio.loudness_output_max_correction_db = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.loudness_output_max_correction_db = 13.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.loudness_output_max_correction_db = 3.0;
        audio.loudness_output_gain_up_db_per_second = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.loudness_output_gain_up_db_per_second = 6.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.loudness_output_gain_up_db_per_second = 0.1;
        audio.loudness_output_gain_down_db_per_second = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.loudness_output_gain_down_db_per_second = 6.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.loudness_output_gain_down_db_per_second = 0.25;
        audio.pause_return_delay_ms = audio.pause_hold_ms - 1.0;
        assert!(super::validate_loudness(&audio).is_err());
        audio.pause_return_delay_ms = 2000.0;
        audio.loudness_scope = "unknown".to_string();
        assert!(super::validate_loudness(&audio).is_err());
        audio.loudness_scope = "live".to_string();
        audio.compressor_ratio = f64::NAN;
        assert!(super::validate_loudness(&audio).is_err());
        audio.compressor_ratio = 3.0;
        audio.pause_threshold_dbfs = -10.0;
        assert!(super::validate_loudness(&audio).is_err());
    }

    #[test]
    fn output_change_requires_restart() {
        let current = PlayoutConfig::default();
        let mut updated = current.clone();
        updated.output.width = 1920;

        assert!(requires_playout_restart(&current, &updated));

        let mut protocol_update = current.clone();
        protocol_update
            .output
            .protocol_options
            .insert("latency".to_string(), "2000000".to_string());
        assert!(requires_playout_restart(&current, &protocol_update));

        let mut metadata_update = current.clone();
        metadata_update
            .output
            .metadata_options
            .insert("title".to_string(), "Example Program".to_string());
        assert!(requires_playout_restart(&current, &metadata_update));
    }
}
