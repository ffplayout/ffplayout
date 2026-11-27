use axum::{Json, extract::State};
use real::RealIp;
use serde::Serialize;

use crate::{
    ARGS,
    api::state::AppState,
    db::handles,
    utils::{errors::ServiceError, setup},
};

pub use crate::{db::models::SetupSettings, utils::setup::SetupRequest};

#[derive(Debug, Serialize)]
pub struct SetupStatus {
    required: bool,
    /// The request must include the console setup token.
    token_required: bool,
    settings: Option<SetupSettings>,
}

pub async fn get_setup_status(
    real_ip: RealIp,
    State(state): State<AppState>,
) -> Result<Json<SetupStatus>, ServiceError> {
    let settings = handles::select_global(&state.pool).await?;
    let user_count = handles::count_users(&state.pool).await?;
    let required = !settings.setup_completed && user_count == 0;

    Ok(Json(SetupStatus {
        required,
        token_required: required
            && setup::setup_token_required(real_ip.ip(), ARGS.allow_local_setup_without_token),
        settings: required.then(|| settings.into()),
    }))
}

pub async fn complete_setup(
    real_ip: RealIp,
    State(state): State<AppState>,
    Json(data): Json<SetupRequest>,
) -> Result<&'static str, ServiceError> {
    setup::ensure_setup_token(
        real_ip.ip(),
        data.setup_token.as_deref(),
        ARGS.allow_local_setup_without_token,
    )?;

    setup::complete_setup(
        &state.pool,
        state.controller.clone(),
        state.mail_queues.clone(),
        state.shutdown.clone(),
        state.system.clone(),
        data,
    )
    .await?;

    Ok("Setup completed")
}
