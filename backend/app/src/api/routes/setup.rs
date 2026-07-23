use std::path::{Component, Path, PathBuf};

use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::task;

use crate::{
    api::state::AppState,
    db::{handles, models::GlobalSettings},
    utils::{channels::initialize_channels, errors::ServiceError},
};

#[derive(Debug, Serialize)]
pub struct SetupStatus {
    required: bool,
    settings: Option<SetupSettings>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SetupSettings {
    logs: String,
    playlists: String,
    public: String,
    storage: String,
    shared: bool,
    smtp_server: String,
    smtp_user: String,
    smtp_password: String,
    smtp_starttls: bool,
    smtp_port: u16,
}

impl From<GlobalSettings> for SetupSettings {
    fn from(settings: GlobalSettings) -> Self {
        Self {
            logs: settings.logs,
            playlists: settings.playlists,
            public: settings.public,
            storage: settings.storage,
            shared: settings.shared,
            smtp_server: settings.smtp_server,
            smtp_user: settings.smtp_user,
            smtp_password: String::new(),
            smtp_starttls: settings.smtp_starttls,
            smtp_port: settings.smtp_port,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    #[serde(flatten)]
    settings: SetupSettings,
    username: String,
    mail: String,
    password: String,
    two_factor: bool,
}

const PROTECTED_SYSTEM_PATHS: &[&str] = &[
    "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/run", "/sbin", "/sys",
    "/tmp", "/usr", "/var",
];
const APPLICATION_SYSTEM_PATHS: &[&str] = &[
    "/usr/share/ffplayout",
    "/var/lib/ffplayout",
    "/var/log/ffplayout",
];

fn validate_setup_paths(settings: &SetupSettings) -> Result<(), ServiceError> {
    [
        ("Logging", settings.logs.as_str()),
        ("Playlist", settings.playlists.as_str()),
        ("Public", settings.public.as_str()),
        ("Storage", settings.storage.as_str()),
    ]
    .into_iter()
    .try_for_each(|(name, path)| validate_setup_path(name, path))
}

fn validate_setup_path(name: &str, value: &str) -> Result<(), ServiceError> {
    let path = Path::new(value.trim());
    if value.trim().is_empty() || !path.is_absolute() || path.parent().is_none() {
        return Err(ServiceError::BadRequest(format!(
            "{name} path must be an absolute directory"
        )));
    }

    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(ServiceError::BadRequest(format!(
            "{name} path must not contain relative components"
        )));
    }

    let is_application_path = APPLICATION_SYSTEM_PATHS
        .iter()
        .any(|allowed| path.starts_with(allowed));
    let is_protected_system_path = PROTECTED_SYSTEM_PATHS
        .iter()
        .any(|protected| path.starts_with(protected));
    if is_protected_system_path && !is_application_path {
        return Err(ServiceError::BadRequest(format!(
            "{name} path must not point to a system directory"
        )));
    }

    Ok(())
}

pub async fn get_setup_status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatus>, ServiceError> {
    let settings = handles::select_global(&state.pool).await?;
    let user_count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM user")
        .fetch_one(&state.pool)
        .await?
        .get("count");
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
    if data.username.trim().is_empty() || data.mail.trim().is_empty() || data.password.is_empty() {
        return Err(ServiceError::BadRequest(
            "Username, email, and password are required".to_string(),
        ));
    }
    validate_setup_paths(&data.settings)?;

    let password = data.password;
    let password_hash = task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
    })
    .await?
    .map_err(|error| ServiceError::Conflict(error.to_string()))?;

    let mut transaction = state.pool.begin().await?;
    let settings = data.settings;
    let channel_path = |path: &str| {
        if settings.shared {
            Path::new(path).join("1").to_string_lossy().to_string()
        } else {
            path.to_string()
        }
    };
    let paths = [
        PathBuf::from(&settings.logs),
        PathBuf::from(channel_path(&settings.public)),
        PathBuf::from(channel_path(&settings.playlists)),
        PathBuf::from(channel_path(&settings.storage)),
    ];

    for path in paths {
        tokio::fs::create_dir_all(&path).await.map_err(|error| {
            ServiceError::Conflict(format!("Cannot create {}: {error}", path.display()))
        })?;
    }

    let result = sqlx::query(
        "UPDATE global SET logs = $1, playlists = $2, public = $3, storage = $4, shared = $5,
        smtp_server = $6, smtp_user = $7, smtp_password = $8, smtp_starttls = $9, smtp_port = $10,
        setup_completed = 0 WHERE id = 1 AND setup_completed = 0
        AND NOT EXISTS (SELECT 1 FROM user)",
    )
    .bind(&settings.logs)
    .bind(&settings.playlists)
    .bind(&settings.public)
    .bind(&settings.storage)
    .bind(settings.shared)
    .bind(&settings.smtp_server)
    .bind(&settings.smtp_user)
    .bind(&settings.smtp_password)
    .bind(settings.smtp_starttls)
    .bind(settings.smtp_port)
    .execute(&mut *transaction)
    .await?;

    if result.rows_affected() != 1 {
        return Err(ServiceError::Conflict(
            "Installation has already been initialized".to_string(),
        ));
    }

    sqlx::query("UPDATE channels SET public = $1, playlists = $2, storage = $3 WHERE id = 1")
        .bind(channel_path(&settings.public))
        .bind(channel_path(&settings.playlists))
        .bind(channel_path(&settings.storage))
        .execute(&mut *transaction)
        .await?;

    sqlx::query(
        "UPDATE configurations SET output_id = (
            SELECT id FROM outputs WHERE channel_id = 1 AND name = 'hls'
        ) WHERE channel_id = 1",
    )
    .execute(&mut *transaction)
    .await?;

    let user_id: i32 = sqlx::query(
        "INSERT INTO user (mail, username, password, role_id, two_factor)
        VALUES ($1, $2, $3, 1, $4) RETURNING id",
    )
    .bind(data.mail.trim())
    .bind(data.username.trim())
    .bind(password_hash)
    .bind(data.two_factor)
    .fetch_one(&mut *transaction)
    .await?
    .get("id");

    sqlx::query("INSERT INTO user_channels (channel_id, user_id) VALUES (1, $1)")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;

    sqlx::query("UPDATE global SET setup_completed = 1 WHERE id = 1")
        .execute(&mut *transaction)
        .await?;

    transaction.commit().await?;

    initialize_channels(
        &state.pool,
        state.controller.clone(),
        state.mail_queues.clone(),
        state.shutdown.clone(),
        state.system.clone(),
        true,
    )
    .await?;

    Ok("Setup completed")
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::validate_setup_path;
    use super::{SetupSettings, validate_setup_paths};

    fn settings(storage: &str) -> SetupSettings {
        SetupSettings {
            logs: "/var/log/ffplayout".to_string(),
            playlists: "/var/lib/ffplayout/playlists".to_string(),
            public: "/usr/share/ffplayout/public".to_string(),
            storage: storage.to_string(),
            shared: false,
            smtp_server: String::new(),
            smtp_user: String::new(),
            smtp_password: String::new(),
            smtp_starttls: false,
            smtp_port: 465,
        }
    }

    #[test]
    fn setup_paths_allow_application_directories_and_custom_data_roots() {
        assert!(validate_setup_paths(&settings("/mnt/media")).is_ok());
    }

    #[test]
    fn setup_paths_reject_protected_system_directories() {
        assert!(validate_setup_paths(&settings("/etc/ffplayout")).is_err());
        assert!(validate_setup_paths(&settings("/var/lib")).is_err());
    }

    #[test]
    fn setup_paths_reject_relative_components() {
        assert!(validate_setup_paths(&settings("/mnt/../etc")).is_err());
        assert!(validate_setup_paths(&settings("media")).is_err());
        assert!(validate_setup_paths(&settings("/")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn setup_paths_allow_windows_drive_and_unc_paths() {
        assert!(validate_setup_path("Logging", r"C:\Users\jonathan\Videos\logs").is_ok());
        assert!(validate_setup_path("Logging", r"\\server\share\logs").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn setup_paths_reject_windows_roots_and_relative_components() {
        assert!(validate_setup_path("Logging", r"C:\").is_err());
        assert!(validate_setup_path("Logging", r"C:\logs\..\system").is_err());
    }
}
