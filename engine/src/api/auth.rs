use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use argon2::{Argon2, PasswordVerifier, password_hash::PasswordHash};
use axum::{Json as AxumJson, extract::State, http::StatusCode, response::IntoResponse};
use chrono::{DateTime, TimeDelta, Utc};
use jsonwebtoken::{self, DecodingKey, EncodingKey, Header, Validation};
use log::*;
use rand::RngExt;
use real::RealIp;
use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, task};

use crate::{
    ARGS,
    api::state::AppState,
    db::{
        GLOBAL_SETTINGS, handles,
        models::{Role, User},
    },
    utils::{config::Mail, errors::ServiceError, mail::send_mail},
};

// Token lifetime
const ACCESS_LIFETIME: i64 = 3;
const REFRESH_LIFETIME: i64 = 30;

// Global storage for verification codes
pub static VERIFICATION_CODES: LazyLock<Arc<Mutex<HashMap<String, VerificationCode>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Clone, Debug)]
pub struct VerificationCode {
    pub code: String,
    pub user: User,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerifyRequest {
    pub username: String,
    pub code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Claims {
    pub id: i32,
    pub channels: Vec<i32>,
    pub username: String,
    pub role: Role,
    exp: i64,
}

impl Claims {
    pub fn new(user: User, role: Role, lifetime: i64) -> Self {
        Self {
            id: user.id,
            channels: user.channel_ids.unwrap_or_default(),
            username: user.username,
            role,
            exp: (Utc::now() + TimeDelta::try_days(lifetime).unwrap()).timestamp(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenRefreshRequest {
    pub refresh: String,
}

/// Create a json web token (JWT)
pub async fn encode_jwt(claims: Claims) -> Result<String, ServiceError> {
    let config = GLOBAL_SETTINGS.get().unwrap();
    let encoding_key = EncodingKey::from_secret(config.secret.clone().unwrap().as_bytes());
    Ok(jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &encoding_key,
    )?)
}

/// Decode a json web token (JWT)
pub async fn decode_jwt(token: &str) -> Result<Claims, ServiceError> {
    let config = GLOBAL_SETTINGS.get().unwrap();
    let decoding_key = DecodingKey::from_secret(config.secret.clone().unwrap().as_bytes());
    jsonwebtoken::decode::<Claims>(token, &decoding_key, &Validation::default())
        .map(|data| data.claims)
        .map_err(|e| ServiceError::Unauthorized(e.to_string()))
}

fn mail_body(verification_code: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    </head>
    <body>
        <div style="padding: 2px 15px;">
            <div>
                <h2>Your verification code</h2>
                <p>Enter this code in the <b>ffplayout</b> verification step to finish signing in:</p>
                <p style="padding: 5px; font-size: 20px; font-weight: bold;">{verification_code}</p>
                <p>This code expires in 5 minutes. If you did not request it, you can ignore this email.</p>
                <div>
                    This message was sent automatically by <b>ffplayout</b>.
                </div>
            </div>
        </div>
    </body>
    </html>"#
    )
}

/// #### User Handling
///
/// **Login**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/auth/login -H "Content-Type: application/json" \
/// -d '{ "username": "<USER>", "password": "<PASS>" }'
/// ```
/// **Response:**
///
/// ```JSON
/// {
///     "access": "<ACCESS TOKEN>",
///     "refresh": "<REFRESH TOKEN>"
/// }
/// ```
pub async fn login(
    real_ip: RealIp,
    State(state): State<AppState>,
    AxumJson(credentials): AxumJson<Credentials>,
) -> Result<impl IntoResponse, ServiceError> {
    let ip = real_ip.ip();
    let username = credentials.username.clone();
    let password = credentials.password.clone();

    match handles::select_login(&state.pool, &username).await {
        Ok(mut user) => {
            if user.username.is_empty() {
                return Ok((
                    StatusCode::FORBIDDEN,
                    AxumJson(serde_json::json!({
                        "detail": "Incorrect credentials!",
                    })),
                )
                    .into_response());
            }

            let role = handles::select_role(&state.pool, &user.role_id.unwrap_or_default()).await?;

            let pass_hash = user.password.clone();
            let cred_password = password.clone();

            user.password = String::new();

            let verified_password = task::spawn_blocking(move || {
                let hash = PasswordHash::new(&pass_hash)?;
                Argon2::default().verify_password(cred_password.as_bytes(), &hash)
            })
            .await?;

            if verified_password.is_ok() {
                let config = GLOBAL_SETTINGS.get().unwrap();

                if let Some(email) = user.mail.clone()
                    && !config.smtp_server.is_empty()
                    && !config.smtp_user.is_empty()
                    && !config.smtp_password.is_empty()
                    && !ARGS.disable_two_factor
                {
                    // Generate 6-digit random code
                    let verification_code: String = (0..6)
                        .map(|_| rand::rng().random_range(0..10).to_string())
                        .collect();

                    // Store code with timestamp
                    let verification_entry = VerificationCode {
                        code: verification_code.clone(),
                        user,
                        role,
                        created_at: Utc::now(),
                    };

                    VERIFICATION_CODES
                        .lock()
                        .await
                        .insert(username.clone(), verification_entry);

                    // Start cleanup task for this code
                    let username_cleanup = username.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_secs(300)).await; // 5 minutes
                        VERIFICATION_CODES.lock().await.remove(&username_cleanup);
                        debug!("Verification code for {username_cleanup} expired and removed");
                    });

                    let text = mail_body(&verification_code);
                    let mail_config = Mail {
                        show: true,
                        subject: format!("Your ffplayout code is: {verification_code}"),
                        smtp_server: config.smtp_server.clone(),
                        smtp_starttls: config.smtp_starttls,
                        smtp_user: config.smtp_user.clone(),
                        smtp_password: config.smtp_password.clone(),
                        smtp_port: config.smtp_port,
                        recipient: email,
                        mail_level: Level::Info,
                        interval: 0,
                    };

                    send_mail(&mail_config, text, true).await?;

                    info!("{ip} Send verification code");

                    return Ok((
                        StatusCode::OK,
                        AxumJson(serde_json::json!({
                            "detail": "Verification code sended to email!",
                        })),
                    )
                        .into_response());
                }

                warn!("No Two-factor authentication!");

                let access_claims = Claims::new(user.clone(), role.clone(), ACCESS_LIFETIME);
                let access_token = encode_jwt(access_claims).await?;
                let refresh_claims = Claims::new(user, role.clone(), REFRESH_LIFETIME);
                let refresh_token = encode_jwt(refresh_claims).await?;

                info!("{ip} user {username} login, with role: {role}");

                return Ok((
                    StatusCode::OK,
                    AxumJson(serde_json::json!({
                        "access": access_token,
                        "refresh": refresh_token,
                    })),
                )
                    .into_response());
            }

            error!("{ip} Wrong password for {username}!");

            Ok((
                StatusCode::FORBIDDEN,
                AxumJson(serde_json::json!({
                    "detail": "Incorrect credentials!",
                })),
            )
                .into_response())
        }
        Err(e) => {
            error!("{ip} Login {username} failed! {e}");

            Ok((
                StatusCode::BAD_REQUEST,
                AxumJson(serde_json::json!({
                    "detail": format!("Login {username} failed!"),
                })),
            )
                .into_response())
        }
    }
}

pub async fn verify(
    real_ip: RealIp,
    AxumJson(request): AxumJson<VerifyRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let ip = real_ip.ip();
    let username = request.username;
    let provided_code = request.code;

    // Check if code exists
    let verification_data = {
        let mut codes = VERIFICATION_CODES.lock().await;

        if let Some(verification) = codes.get(&username) {
            // Check if code is still valid (max 5 minutes)
            let elapsed = Utc::now().signed_duration_since(verification.created_at);
            if elapsed.num_minutes() > 5 {
                codes.remove(&username);
                return Ok((
                    StatusCode::BAD_REQUEST,
                    AxumJson(serde_json::json!({
                        "detail": "Verification code expired!",
                    })),
                )
                    .into_response());
            }

            // Check if code is correct
            if verification.code != provided_code {
                return Ok((
                    StatusCode::FORBIDDEN,
                    AxumJson(serde_json::json!({
                        "detail": "Invalid verification code!",
                    })),
                )
                    .into_response());
            }

            // Code is valid, remove it and return data
            let data = verification.clone();
            codes.remove(&username);
            Some(data)
        } else {
            None
        }
    };

    match verification_data {
        Some(verification) => {
            let user = verification.user;
            let role = verification.role;

            // Generate JWT tokens
            let access_claims = Claims::new(user.clone(), role.clone(), ACCESS_LIFETIME);
            let access_token = encode_jwt(access_claims).await?;
            let refresh_claims = Claims::new(user, role.clone(), REFRESH_LIFETIME);
            let refresh_token = encode_jwt(refresh_claims).await?;

            info!(
                "{ip} User {username} verified successfully, with role: {}",
                role
            );

            Ok((
                StatusCode::OK,
                AxumJson(serde_json::json!({
                    "access": access_token,
                    "refresh": refresh_token,
                })),
            )
                .into_response())
        }
        None => {
            error!("{ip} No verification code found for {username}");
            Ok((
                StatusCode::FORBIDDEN,
                AxumJson(serde_json::json!({
                    "detail": "No verification code found or code expired!",
                })),
            )
                .into_response())
        }
    }
}

/// **Refresh token**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/auth/refresh -H "Content-Type: application/json" \
/// -d '{ "refresh": "REFRESH TOKEN>" }'
/// ```
/// **Response:**
///
/// ```JSON
/// {
///     "access": "<ACCESS TOKEN>",
/// }
/// ```
pub async fn refresh(
    State(state): State<AppState>,
    AxumJson(data): AxumJson<TokenRefreshRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let refresh_t = &data.refresh;

    match decode_jwt(refresh_t).await {
        Ok(claims) => {
            let user_id = claims.id;
            let role = claims.role;

            if let Ok(user) = handles::select_user(&state.pool, user_id).await {
                let access_claims = Claims::new(user.clone(), role.clone(), ACCESS_LIFETIME);
                let access_token = encode_jwt(access_claims).await?;

                info!("user {} refresh, with role: {role}", user.username);

                Ok((
                    StatusCode::OK,
                    AxumJson(serde_json::json!({
                        "access": access_token,
                    })),
                ))
            } else {
                Ok((
                    StatusCode::UNAUTHORIZED,
                    AxumJson(serde_json::json!({
                        "detail": "Invalid user in refresh token",
                    })),
                ))
            }
        }
        Err(_) => Ok((
            StatusCode::FORBIDDEN,
            AxumJson(serde_json::json!({
                "detail": "Invalid refresh token",
            })),
        )),
    }
}
