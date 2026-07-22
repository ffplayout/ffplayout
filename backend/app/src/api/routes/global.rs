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
    smtp_server: String,
    smtp_user: String,
    smtp_password_set: bool,
    smtp_starttls: bool,
    smtp_port: u16,
}

impl From<GlobalSettings> for GlobalSettingsResponse {
    fn from(settings: GlobalSettings) -> Self {
        Self {
            smtp_server: settings.smtp_server,
            smtp_user: settings.smtp_user,
            smtp_password_set: !settings.smtp_password.is_empty(),
            smtp_starttls: settings.smtp_starttls,
            smtp_port: settings.smtp_port,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateGlobalSettings {
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
    settings.smtp_server = data.smtp_server;
    settings.smtp_user = data.smtp_user;
    settings.smtp_starttls = data.smtp_starttls;
    settings.smtp_port = data.smtp_port;

    if let Some(password) = data.smtp_password.filter(|password| !password.is_empty()) {
        settings.smtp_password = password;
    }

    handles::update_global_runtime_settings(&state.pool, settings.clone()).await?;

    // Keep per-channel recipients intact while applying shared SMTP settings
    // to the queues that are already running.
    for queue in state.mail_queues.lock().await.iter() {
        let mut queue = queue.lock().await;
        queue.config.smtp_server.clone_from(&settings.smtp_server);
        queue.config.smtp_starttls = settings.smtp_starttls;
        queue.config.smtp_user.clone_from(&settings.smtp_user);
        queue
            .config
            .smtp_password
            .clone_from(&settings.smtp_password);
        queue.config.smtp_port = settings.smtp_port;
        queue.config.show =
            !settings.smtp_password.is_empty() && settings.smtp_server != "mail.example.org";
    }

    Ok(Json(settings.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert!(value.get("logs").is_none());
        assert!(value.get("playlists").is_none());
        assert!(value.get("public").is_none());
        assert!(value.get("storage").is_none());
        assert!(value.get("shared").is_none());
        assert_eq!(value["smtp_password_set"], true);
    }

    #[test]
    fn global_settings_update_rejects_path_fields() {
        let request = json!({
            "logs": "/tmp/ffplayout",
            "smtp_server": "mail.example.org",
            "smtp_user": "ffplayout@example.org",
            "smtp_starttls": false,
            "smtp_port": 465,
        });

        assert!(serde_json::from_value::<UpdateGlobalSettings>(request).is_err());
    }
}
