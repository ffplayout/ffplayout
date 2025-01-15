use actix_web::{error::ErrorUnauthorized, http::StatusCode, web, Error, Responder};
use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
use chrono::{TimeDelta, Utc};
use jsonwebtoken::{self, DecodingKey, EncodingKey, Header, Validation};
use log::*;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

use crate::{
    db::{handles, models::Role, models::User, GLOBAL_SETTINGS},
    utils::errors::ServiceError,
};

// Token lifetime
const ACCESS_LIFETIME: i64 = 3;
const REFRESH_LIFETIME: i64 = 30;

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
pub async fn decode_jwt(token: &str) -> Result<Claims, Error> {
    let config = GLOBAL_SETTINGS.get().unwrap();
    let decoding_key = DecodingKey::from_secret(config.secret.clone().unwrap().as_bytes());
    jsonwebtoken::decode::<Claims>(token, &decoding_key, &Validation::default())
        .map(|data| data.claims)
        .map_err(|e| ErrorUnauthorized(e.to_string()))
}

pub async fn authorize(
    pool: &Pool<Sqlite>,
    credentials: Credentials,
) -> Result<impl Responder, ServiceError> {
    let username = credentials.username.clone();
    let password = credentials.password.clone();

    match handles::select_login(pool, &username).await {
        Ok(mut user) => {
            let role = handles::select_role(pool, &user.role_id.unwrap_or_default()).await?;

            let pass_hash = user.password.clone();
            let cred_password = password.clone();

            user.password = String::new();

            let verified_password = web::block(move || {
                let hash = PasswordHash::new(&pass_hash)?;
                Argon2::default().verify_password(cred_password.as_bytes(), &hash)
            })
            .await?;

            if verified_password.is_ok() {
                let access_claims = Claims::new(user.clone(), role.clone(), ACCESS_LIFETIME);
                let access_token = encode_jwt(access_claims).await?;
                let refresh_claims = Claims::new(user, role.clone(), REFRESH_LIFETIME);
                let refresh_token = encode_jwt(refresh_claims).await?;

                info!("user {} login, with role: {role}", username);

                Ok(web::Json(serde_json::json!({
                    "access": access_token,
                    "refresh": refresh_token,
                }))
                .customize()
                .with_status(StatusCode::OK))
            } else {
                error!("Wrong password for {username}!");

                Ok(web::Json(serde_json::json!({
                    "detail": "Incorrect credentials!",
                }))
                .customize()
                .with_status(StatusCode::FORBIDDEN))
            }
        }
        Err(e) => {
            error!("Login {username} failed! {e}");

            Ok(web::Json(serde_json::json!({
                "detail": format!("Login {username} failed!"),
            }))
            .customize()
            .with_status(StatusCode::BAD_REQUEST))
        }
    }
}

pub async fn refresh(
    pool: &Pool<Sqlite>,
    data: TokenRefreshRequest,
) -> Result<impl Responder, ServiceError> {
    let refresh_token = &data.refresh;

    match decode_jwt(refresh_token).await {
        Ok(claims) => {
            let user_id = claims.id;
            let role = claims.role;

            if let Ok(user) = handles::select_user(pool, user_id).await {
                let access_claims = Claims::new(user.clone(), role.clone(), ACCESS_LIFETIME);
                let access_token = encode_jwt(access_claims).await?;

                info!("user {} refresh, with role: {role}", user.username);

                Ok(web::Json(serde_json::json!({
                    "access": access_token
                }))
                .customize()
                .with_status(StatusCode::OK))
            } else {
                Ok(web::Json(serde_json::json!({
                    "detail": "Invalid user in refresh token",
                }))
                .customize()
                .with_status(StatusCode::UNAUTHORIZED))
            }
        }
        Err(_) => Ok(web::Json(serde_json::json!({
            "detail": "Invalid refresh token",
        }))
        .customize()
        .with_status(StatusCode::BAD_REQUEST)),
    }
}
