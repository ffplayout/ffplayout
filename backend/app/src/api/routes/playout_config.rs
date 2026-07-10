use std::path::Path;

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
}

#[derive(Debug, Serialize)]
pub struct OutputCodecOptions {
    pub video: Vec<CodecOption>,
    pub audio: Vec<CodecOption>,
}

#[derive(Debug, Serialize)]
pub struct PlayoutCodecOptions {
    pub hls: OutputCodecOptions,
    pub rtmp: OutputCodecOptions,
    pub srt: OutputCodecOptions,
    pub udp: OutputCodecOptions,
}

fn codec_option(codec: &ff_engine::FfmpegCodec) -> CodecOption {
    CodecOption {
        name: codec.name.clone(),
        display_name: codec.display_name.clone(),
        codec_id: codec.codec_id.clone(),
        hardware: codec.hardware,
    }
}

fn output_codec_options(target: ff_engine::FfmpegOutputTarget) -> OutputCodecOptions {
    let capabilities = ff_engine::ffmpeg_capabilities();

    OutputCodecOptions {
        video: capabilities
            .video_codecs_for(target)
            .iter()
            .filter(|codec| !codec.hardware)
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
) -> Result<Json<&'static str>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;
    user.ensure_channel_or_admin(id)?;

    let manager = {
        let guard = state.controller.read().await;
        guard.get(id)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;
    let p = manager.channel.lock().await.storage.clone();
    let storage = Path::new(&p);
    let config_id = manager.config.read().await.general.id;

    let (_, _, logo) = norm_abs_path(storage, &data.processing.logo)?;
    let (_, _, filler) = norm_abs_path(storage, &data.storage.filler)?;

    data.processing.logo = logo;
    data.storage.filler = filler;
    if let Some(preset_id) = data.text.preset_id {
        let preset = handles::select_preset(&state.pool, id, preset_id)
            .await
            .map_err(|_| ServiceError::BadRequest("invalid text preset".to_string()))?;
        if !preset.use_filename {
            return Err(ServiceError::BadRequest(
                "automatic text preset must use the clip filename".to_string(),
            ));
        }
    }
    data.processing
        .hls_subtitle()
        .map_err(ServiceError::BadRequest)?;
    ff_engine::AudioEffectsControl::new(data.processing.volume)
        .map_err(|error| ServiceError::BadRequest(error.to_string()))?;
    data.output.validate().map_err(ServiceError::BadRequest)?;

    let is_hls = data.output.mode == OutputMode::HLS;
    let is_encoded = matches!(data.output.mode, OutputMode::HLS | OutputMode::Stream);
    let is_crf = is_encoded && data.output.rate_control == "crf";
    handles::update_output(
        &state.pool,
        data.output.id,
        id,
        &data.output.hls_variants.join(";"),
        &data.output.stream_url,
        (data.output.mode == OutputMode::Stream).then_some(match data.output.stream_type {
            crate::utils::config::StreamType::Rtmp => "rtmp",
            crate::utils::config::StreamType::Srt => "srt",
            crate::utils::config::StreamType::Udp => "udp",
        }),
        is_hls.then_some(data.output.hls_playlist_name.as_str()),
        is_hls.then_some(i64::from(data.output.hls_segment_duration)),
        is_hls.then_some(i64::from(data.output.hls_list_size)),
        data.output.desktop_fullscreen,
        i64::from(data.output.width),
        i64::from(data.output.height),
        data.output.fps,
        is_encoded.then_some(data.output.video_preset.as_str()),
        is_encoded.then_some(data.output.video_codec.as_str()),
        is_encoded.then_some(data.output.audio_codec.as_str()),
        is_encoded.then_some(data.output.rate_control.as_str()),
        is_crf.then_some(i64::from(data.output.video_quality)),
        is_encoded.then_some(i64::from(data.output.video_maxrate)),
        is_encoded.then_some(i64::from(data.output.audio_bitrate)),
    )
    .await?;
    handles::update_configuration(&state.pool, config_id, data).await?;
    let new_config = get_config(&state.pool, id).await?;
    let mut queues = state.mail_queues.lock().await;

    for queue in queues.iter_mut() {
        let mut queue_lock = queue.lock().await;

        if queue_lock.id == id {
            if queue_lock.config.recipient != new_config.mail.recipient {
                queue_lock.clear_raw();
            }

            queue_lock.update(new_config.mail.clone());
            break;
        }
    }

    manager.update_config(new_config).await;

    Ok(Json("Update success"))
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

    if let Ok(outputs) = handles::select_outputs(&state.pool, id).await {
        return Ok(Json(outputs));
    }

    Err(ServiceError::InternalServerError)
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
        hls: output_codec_options(ff_engine::FfmpegOutputTarget::Hls),
        rtmp: output_codec_options(ff_engine::FfmpegOutputTarget::Rtmp),
        srt: output_codec_options(ff_engine::FfmpegOutputTarget::Srt),
        udp: output_codec_options(ff_engine::FfmpegOutputTarget::Udp),
    }))
}
