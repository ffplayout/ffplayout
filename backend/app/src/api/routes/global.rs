use axum::{Json, extract::State};
use protect_axum::authorities::AuthDetails;
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{GlobalSettings, Role},
    },
    utils::errors::ServiceError,
};

#[derive(Debug, Serialize)]
pub struct GlobalSettingsResponse {
    logs: String,
    playlists: String,
    public: String,
    storage: String,
    shared: bool,
    smtp_server: String,
    smtp_user: String,
    smtp_password_set: bool,
    smtp_starttls: bool,
    smtp_port: u16,
}

impl From<GlobalSettings> for GlobalSettingsResponse {
    fn from(settings: GlobalSettings) -> Self {
        Self {
            logs: settings.logs,
            playlists: settings.playlists,
            public: settings.public,
            storage: settings.storage,
            shared: settings.shared,
            smtp_server: settings.smtp_server,
            smtp_user: settings.smtp_user,
            smtp_password_set: !settings.smtp_password.is_empty(),
            smtp_starttls: settings.smtp_starttls,
            smtp_port: settings.smtp_port,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateGlobalSettings {
    logs: String,
    playlists: String,
    public: String,
    storage: String,
    shared: bool,
    smtp_server: String,
    smtp_user: String,
    #[serde(default)]
    smtp_password: Option<String>,
    smtp_starttls: bool,
    smtp_port: u16,
}

pub async fn get_global(
    State(state): State<AppState>,
    _user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<GlobalSettingsResponse>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;

    Ok(Json(handles::select_global(&state.pool).await?.into()))
}

pub async fn update_global(
    State(state): State<AppState>,
    _user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<UpdateGlobalSettings>,
) -> Result<Json<GlobalSettingsResponse>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;

    let mut settings = handles::select_global(&state.pool).await?;
    settings.logs = data.logs;
    settings.playlists = data.playlists;
    settings.public = data.public;
    settings.storage = data.storage;
    settings.shared = data.shared;
    settings.smtp_server = data.smtp_server;
    settings.smtp_user = data.smtp_user;
    settings.smtp_starttls = data.smtp_starttls;
    settings.smtp_port = data.smtp_port;

    if let Some(password) = data.smtp_password.filter(|password| !password.is_empty()) {
        settings.smtp_password = password;
    }

    handles::update_global(&state.pool, settings.clone()).await?;

    Ok(Json(settings.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_settings_response_excludes_secrets() {
        let settings = GlobalSettings {
            secret: Some("jwt-secret".to_string()),
            smtp_password: "smtp-password".to_string(),
            ..Default::default()
        };

        let value = serde_json::to_value(GlobalSettingsResponse::from(settings)).unwrap();

        assert!(value.get("secret").is_none());
        assert!(value.get("smtp_password").is_none());
        assert_eq!(value["smtp_password_set"], true);
    }
}
