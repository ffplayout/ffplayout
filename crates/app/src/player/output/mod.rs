mod instances;
mod playout;

use crate::{player::controller::ChannelManager, utils::errors::ServiceError};

/// Player entry point backed by crates/engine.
pub async fn player(manager: ChannelManager) -> Result<(), ServiceError> {
    playout::player(manager).await
}

/// Queue an atomic snapshot for the active parallel runtime.
pub(crate) fn update_outputs(
    manager: &ChannelManager,
    config: &crate::utils::config::PlayoutConfig,
) -> Result<(), ServiceError> {
    let mut program = playout::engine_output_config(
        config,
        manager.audio_effects.clone(),
        manager.live_loudness.clone(),
        manager.loudness_meter.clone(),
        manager.audio_level.clone(),
        manager.text_overlay.clone(),
        playout::desktop_control_callback(manager.clone()),
    )?;
    program.output_status = manager.output_status.clone();
    program.output_timing_diagnostics = crate::ARGS.timing_diagnostics;
    let mut attachments = Vec::new();
    let recording_owner = manager
        .running_config()
        .map_or(config.output.id, |running| running.output.id);
    instances::configure_destinations(config, &mut program, &mut attachments, recording_owner)?;
    manager
        .output_status
        .update_outputs(program.destinations)
        .map_err(|error| ServiceError::Conflict(error.to_string()))
}
