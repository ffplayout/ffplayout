use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i64,
    #[sqlx(default)]
    pub email: Option<String>,
    pub username: String,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub password: String,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub salt: Option<String>,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub role_id: Option<i64>,
    #[sqlx(default)]
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoginUser {
    pub id: i64,
    pub username: String,
}

impl LoginUser {
    pub fn new(id: i64, username: String) -> Self {
        Self { id, username }
    }
}

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Settings {
    pub id: i64,
    pub channel_name: String,
    pub preview_url: String,
    pub settings_path: String,
    pub extra_extensions: String,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub secret: String,
}
