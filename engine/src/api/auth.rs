use actix_web::error::ErrorUnauthorized;
use actix_web::Error;
use chrono::{TimeDelta, Utc};
use jsonwebtoken::{self, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::{
    db::{models::Role, GLOBAL_SETTINGS},
    utils::errors::ServiceError,
};

// Token lifetime
const JWT_EXPIRATION_DAYS: i64 = 7;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Claims {
    pub id: i32,
    pub channels: Vec<i32>,
    pub username: String,
    pub role: Role,
    exp: i64,
}

impl Claims {
    pub fn new(id: i32, channels: Vec<i32>, username: String, role: Role) -> Self {
        Self {
            id,
            channels,
            username,
            role,
            exp: (Utc::now() + TimeDelta::try_days(JWT_EXPIRATION_DAYS).unwrap()).timestamp(),
        }
    }
}

/// Create a json web token (JWT)
pub async fn create_jwt(claims: Claims) -> Result<String, ServiceError> {
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
