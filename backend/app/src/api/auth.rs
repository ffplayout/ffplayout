use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString, rand_core::OsRng},
};
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

const ACCESS_LIFETIME_MINUTES: i64 = 45;
const REFRESH_LIFETIME_DAYS: i64 = 14;
const ACCESS_LIFETIME_SECONDS: i64 = ACCESS_LIFETIME_MINUTES * 60;

// Global storage for verification codes
pub static VERIFICATION_CODES: LazyLock<Arc<Mutex<HashMap<String, VerificationCode>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// A throwaway Argon2 hash used to equalize login timing when the username does
/// not exist, so a failed lookup costs about as much as a real verification.
static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(b"ffplayout-dummy-password", &salt)
        .expect("dummy password hash must be valid")
        .to_string()
});

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
    pub token_type: TokenType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    #[serde(default)]
    iat: i64,
    exp: i64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    Access,
    Refresh,
}

impl Claims {
    fn access(user: User, role: Role) -> Self {
        let now = Utc::now();
        Self {
            id: user.id,
            channels: user.channel_ids.unwrap_or_default(),
            username: user.username,
            role,
            token_type: TokenType::Access,
            jti: None,
            iat: now.timestamp(),
            exp: (now + TimeDelta::minutes(ACCESS_LIFETIME_MINUTES)).timestamp(),
        }
    }

    fn refresh(user: User, role: Role, jti: String) -> Self {
        let now = Utc::now();
        Self {
            id: user.id,
            channels: user.channel_ids.unwrap_or_default(),
            username: user.username,
            role,
            token_type: TokenType::Refresh,
            jti: Some(jti),
            iat: now.timestamp(),
            exp: (now + TimeDelta::days(REFRESH_LIFETIME_DAYS)).timestamp(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TokenPair {
    access: String,
    refresh: String,
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

fn decode_jwt_with_type(token: &str, expected: TokenType) -> Result<Claims, ServiceError> {
    let config = GLOBAL_SETTINGS.get().unwrap();
    let decoding_key = DecodingKey::from_secret(config.secret.clone().unwrap().as_bytes());
    let claims = jsonwebtoken::decode::<Claims>(token, &decoding_key, &Validation::default())
        .map(|data| data.claims)
        .map_err(|e| ServiceError::Unauthorized(e.to_string()))?;

    if claims.token_type != expected {
        return Err(ServiceError::Unauthorized(format!(
            "Expected {expected:?} token"
        )));
    }

    if claims.iat <= 0 || claims.exp <= claims.iat {
        return Err(ServiceError::Unauthorized("Invalid token lifetime".into()));
    }
    if expected == TokenType::Access && claims.exp - claims.iat > ACCESS_LIFETIME_SECONDS {
        return Err(ServiceError::Unauthorized(
            "Access token lifetime exceeds the allowed maximum".into(),
        ));
    }
    if expected == TokenType::Refresh && claims.jti.is_none() {
        return Err(ServiceError::Unauthorized(
            "Refresh token has no identifier".into(),
        ));
    }

    Ok(claims)
}

/// Decode an access token used to authorize API requests.
pub async fn decode_jwt(token: &str) -> Result<Claims, ServiceError> {
    decode_jwt_with_type(token, TokenType::Access)
}

/// Decode a refresh token used only by the refresh endpoint.
pub async fn decode_refresh_jwt(token: &str) -> Result<Claims, ServiceError> {
    decode_jwt_with_type(token, TokenType::Refresh)
}

async fn issue_token_pair(
    pool: &sqlx::SqlitePool,
    user: User,
    role: Role,
) -> Result<TokenPair, ServiceError> {
    let jti = uuid::Uuid::new_v4().to_string();
    let user_id = user.id;
    let access = encode_jwt(Claims::access(user.clone(), role.clone())).await?;
    let refresh_claims = Claims::refresh(user, role, jti.clone());
    let expires_at = refresh_claims.exp;
    let refresh = encode_jwt(refresh_claims).await?;
    handles::insert_refresh_token(
        pool,
        &jti,
        &jti,
        user_id,
        expires_at,
        Utc::now().timestamp(),
    )
    .await?;

    Ok(TokenPair { access, refresh })
}

async fn rotate_token_pair(
    pool: &sqlx::SqlitePool,
    old_claims: &Claims,
    user: User,
    role: Role,
) -> Result<Option<TokenPair>, ServiceError> {
    let Some(old_jti) = old_claims.jti.as_deref() else {
        return Ok(None);
    };
    let new_jti = uuid::Uuid::new_v4().to_string();
    let access = encode_jwt(Claims::access(user.clone(), role.clone())).await?;
    let refresh_claims = Claims::refresh(user, role, new_jti.clone());
    let expires_at = refresh_claims.exp;
    let refresh = encode_jwt(refresh_claims).await?;
    let rotation = handles::rotate_refresh_token(
        pool,
        old_jti,
        &new_jti,
        old_claims.id,
        expires_at,
        Utc::now().timestamp(),
    )
    .await?;

    Ok((rotation == handles::RefreshRotation::Rotated).then_some(TokenPair { access, refresh }))
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
                // SMTP settings can be created or changed while the server is
                // running, so do not use the startup-only global settings here.
                let config = handles::select_global(&state.pool).await?;

                if let Some(email) = user.mail.clone()
                    && !config.smtp_server.is_empty()
                    && !config.smtp_user.is_empty()
                    && !config.smtp_password.is_empty()
                    && user.two_factor
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

                    {
                        let mut codes = VERIFICATION_CODES.lock().await;
                        // Drop any expired codes on the way in instead of
                        // spawning a per-login timer task; that avoids
                        // accumulating sleeping tasks under login floods and the
                        // race where a stale timer removes a freshly issued code.
                        let now = Utc::now();
                        codes.retain(|_, entry| {
                            now.signed_duration_since(entry.created_at).num_minutes() <= 5
                        });
                        codes.insert(username.clone(), verification_entry);
                    }

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

                let tokens = issue_token_pair(&state.pool, user, role.clone()).await?;

                info!("{ip} user {username} login, with role: {role}");

                return Ok((
                    StatusCode::OK,
                    AxumJson(serde_json::json!({
                        "access": tokens.access,
                        "refresh": tokens.refresh,
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
            // Spend roughly the same time as a real password verification so an
            // attacker cannot tell existing usernames apart by timing, and
            // return the exact same response as a wrong password to avoid user
            // enumeration through the status code or message.
            let cred_password = password.clone();
            let _ = task::spawn_blocking(move || {
                let hash = PasswordHash::new(&DUMMY_PASSWORD_HASH)?;
                Argon2::default().verify_password(cred_password.as_bytes(), &hash)
            })
            .await;

            error!("{ip} Login {username} failed! {e}");

            Ok((
                StatusCode::FORBIDDEN,
                AxumJson(serde_json::json!({
                    "detail": "Incorrect credentials!",
                })),
            )
                .into_response())
        }
    }
}

pub async fn verify(
    real_ip: RealIp,
    State(state): State<AppState>,
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

            let tokens = issue_token_pair(&state.pool, user, role.clone()).await?;

            info!(
                "{ip} User {username} verified successfully, with role: {}",
                role
            );

            Ok((
                StatusCode::OK,
                AxumJson(serde_json::json!({
                    "access": tokens.access,
                    "refresh": tokens.refresh,
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
///     "refresh": "<ROTATED REFRESH TOKEN>"
/// }
/// ```
pub async fn refresh(
    State(state): State<AppState>,
    AxumJson(data): AxumJson<TokenRefreshRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let refresh_t = &data.refresh;

    match decode_refresh_jwt(refresh_t).await {
        Ok(claims) => {
            let user_id = claims.id;

            if let Ok(user) = handles::select_user(&state.pool, user_id).await {
                let Some(role_id) = user.role_id else {
                    return Ok((
                        StatusCode::UNAUTHORIZED,
                        AxumJson(serde_json::json!({
                            "detail": "Invalid role for user in refresh token",
                        })),
                    ));
                };
                let Ok(role) = handles::select_role(&state.pool, &role_id).await else {
                    return Ok((
                        StatusCode::UNAUTHORIZED,
                        AxumJson(serde_json::json!({
                            "detail": "Invalid role for user in refresh token",
                        })),
                    ));
                };
                let Some(tokens) =
                    rotate_token_pair(&state.pool, &claims, user.clone(), role.clone()).await?
                else {
                    return Ok((
                        StatusCode::FORBIDDEN,
                        AxumJson(serde_json::json!({
                            "detail": "Invalid refresh token",
                        })),
                    ));
                };

                info!("user {} refresh, with role: {role}", user.username);

                Ok((
                    StatusCode::OK,
                    AxumJson(serde_json::json!({
                        "access": tokens.access,
                        "refresh": tokens.refresh,
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

/// Revoke the refresh-token family for the current session.
pub async fn logout(
    State(state): State<AppState>,
    AxumJson(data): AxumJson<TokenRefreshRequest>,
) -> Result<StatusCode, ServiceError> {
    if let Ok(claims) = decode_refresh_jwt(&data.refresh).await
        && let Some(jti) = claims.jti
    {
        handles::revoke_refresh_family(&state.pool, &jti, claims.id, Utc::now().timestamp())
            .await?;
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> User {
        User {
            id: 1,
            mail: None,
            username: "admin".into(),
            password: String::new(),
            role_id: Some(1),
            channel_ids: Some(vec![1]),
            token: None,
            two_factor: false,
        }
    }

    #[test]
    fn token_lifetimes_match_session_policy() {
        let access = Claims::access(user(), Role::GlobalAdmin);
        assert_eq!(access.exp - access.iat, ACCESS_LIFETIME_SECONDS);
        assert!(access.jti.is_none());

        let refresh = Claims::refresh(user(), Role::GlobalAdmin, "token-id".into());
        assert_eq!(
            refresh.exp - refresh.iat,
            TimeDelta::days(REFRESH_LIFETIME_DAYS).num_seconds()
        );
        assert_eq!(refresh.jti.as_deref(), Some("token-id"));
    }
}
