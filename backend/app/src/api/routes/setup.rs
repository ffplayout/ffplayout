use std::path::{Path, PathBuf};

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

    transaction.commit().await?;

    initialize_channels(
        &state.pool,
        state.controller.clone(),
        state.mail_queues.clone(),
        state.system.clone(),
        true,
    )
    .await?;
    handles::mark_setup_completed(&state.pool).await?;

    Ok("Setup completed")
}
