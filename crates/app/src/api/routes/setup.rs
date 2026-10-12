use axum::{Json, extract::State};
use serde::Serialize;

use crate::{
    api::state::AppState,
    db::handles,
    utils::{errors::ServiceError, setup},
};

pub use crate::{db::models::SetupSettings, utils::setup::SetupRequest};

#[derive(Debug, Serialize)]
pub struct SetupStatus {
    required: bool,
    settings: Option<SetupSettings>,
}

pub async fn get_setup_status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatus>, ServiceError> {
    let settings = handles::select_global(&state.pool).await?;
    let user_count = handles::count_users(&state.pool).await?;
    let required = !settings.setup_completed && user_count == 0;

    Ok(Json(SetupStatus {
        required,
        settings: required.then(|| settings.into()),
    }))
}

pub async fn complete_setup(
    State(state): State<AppState>,
    Json(data): Json<SetupRequest>,
) -> Result<&'static str, ServiceError> {
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
