use actix_web::error::ErrorUnauthorized;
use actix_web::Error;
use chrono::{Duration, Utc};
use jsonwebtoken::{self, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::api::utils::GlobalSettings;

// Token lifetime and Secret key are hardcoded for clarity
const JWT_EXPIRATION_MINUTES: i64 = 60;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Claims {
    pub id: i64,
    pub username: String,
    pub permissions: Vec<String>,
    exp: i64,
}

impl Claims {
    pub fn new(id: i64, username: String, permissions: Vec<String>) -> Self {
        Self {
            id,
            username,
            permissions,
            exp: (Utc::now() + Duration::minutes(JWT_EXPIRATION_MINUTES)).timestamp(),
        }
    }
}

/// Create a json web token (JWT)
pub fn create_jwt(claims: Claims) -> Result<String, Error> {
    let config = GlobalSettings::global();
    let encoding_key = EncodingKey::from_secret(config.secret.as_bytes());
    jsonwebtoken::encode(&Header::default(), &claims, &encoding_key)
        .map_err(|e| ErrorUnauthorized(e.to_string()))
}

/// Decode a json web token (JWT)
pub async fn decode_jwt(token: &str) -> Result<Claims, Error> {
    let config = GlobalSettings::global();
    let decoding_key = DecodingKey::from_secret(config.secret.as_bytes());
    jsonwebtoken::decode::<Claims>(token, &decoding_key, &Validation::default())
        .map(|data| data.claims)
        .map_err(|e| ErrorUnauthorized(e.to_string()))
}
