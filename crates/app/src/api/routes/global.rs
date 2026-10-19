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
    notification_server: String,
    notification_token_set: bool,
}

impl From<GlobalSettings> for GlobalSettingsResponse {
    fn from(settings: GlobalSettings) -> Self {
        Self {
            smtp_server: settings.smtp_server,
            smtp_user: settings.smtp_user,
            smtp_password_set: !settings.smtp_password.is_empty(),
            smtp_starttls: settings.smtp_starttls,
            smtp_port: settings.smtp_port,
            notification_server: settings.notification_server,
            notification_token_set: !settings.notification_token.is_empty(),
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
    #[serde(default)]
    notification_server: String,
    #[serde(default)]
    notification_token: Option<String>,
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
    settings.notification_server = data.notification_server.trim_end_matches('/').to_string();

    if !settings.notification_server.is_empty() {
        let url = reqwest::Url::parse(&settings.notification_server).map_err(|error| {
            ServiceError::BadRequest(format!("invalid notification server: {error}"))
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ServiceError::BadRequest(
                "notification server must use HTTP or HTTPS".to_string(),
            ));
        }
    }

    if let Some(password) = data.smtp_password.filter(|password| !password.is_empty()) {
        settings.smtp_password = password;
    }
    if let Some(token) = data.notification_token.filter(|token| !token.is_empty()) {
        settings.notification_token = token;
    }

    handles::update_global_runtime_settings(&state.pool, settings.clone()).await?;

    // The playout API serves the configuration cached by each channel manager.
    // Keep its global mail and notification fields in sync so newly enabled
    // services are visible immediately without restarting ffplayout.
    let managers = state.controller.read().await.managers.clone();
    for manager in managers {
        let mut config = manager.config.write().await;
        config.mail.smtp_server.clone_from(&settings.smtp_server);
        config.mail.smtp_starttls = settings.smtp_starttls;
        config.mail.smtp_user.clone_from(&settings.smtp_user);
        config
            .mail
            .smtp_password
            .clone_from(&settings.smtp_password);
        config.mail.smtp_port = settings.smtp_port;
        config.mail.show =
            !settings.smtp_password.is_empty() && settings.smtp_server != "mail.example.org";
        config
            .notification
            .server
            .clone_from(&settings.notification_server);
        config
            .notification
            .token
            .clone_from(&settings.notification_token);
        config.notification.show = !settings.notification_server.is_empty();
    }

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
        queue
            .notification
            .server
            .clone_from(&settings.notification_server);
        queue
            .notification
            .token
            .clone_from(&settings.notification_token);
        queue.notification.show = !settings.notification_server.is_empty();
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
            notification_token: "notification-token".to_string(),
            ..Default::default()
        };

        let value = serde_json::to_value(GlobalSettingsResponse::from(settings)).unwrap();

        assert!(value.get("secret").is_none());
        assert!(value.get("smtp_password").is_none());
        assert!(value.get("notification_token").is_none());
        assert!(value.get("logs").is_none());
        assert!(value.get("playlists").is_none());
        assert!(value.get("public").is_none());
        assert!(value.get("storage").is_none());
        assert!(value.get("shared").is_none());
        assert_eq!(value["smtp_password_set"], true);
        assert_eq!(value["notification_token_set"], true);
    }

    #[test]
    fn global_settings_update_rejects_path_fields() {
        let request = json!({
            "logs": "/tmp/ffplayout",
            "smtp_server": "mail.example.org",
            "smtp_user": "ffplayout@example.org",
            "smtp_starttls": false,
            "smtp_port": 465,
            "notification_server": "",
        });

        assert!(serde_json::from_value::<UpdateGlobalSettings>(request).is_err());
    }
}
