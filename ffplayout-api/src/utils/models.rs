use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i64,
    #[sqlx(default)]
    pub mail: Option<String>,
    pub username: String,
    #[sqlx(default)]
    #[serde(skip_serializing, default = "empty_string")]
    pub password: String,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub salt: Option<String>,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub role_id: Option<i64>,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub channel_id: Option<i64>,
    #[sqlx(default)]
    pub token: Option<String>,
}

fn empty_string() -> String {
    "".to_string()
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
#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow)]
pub struct TextPreset {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i64,
    pub channel_id: i64,
    pub name: String,
    pub text: String,
    pub x: String,
    pub y: String,
    pub fontsize: String,
    pub line_spacing: String,
    pub fontcolor: String,
    pub r#box: String,
    pub boxcolor: String,
    pub boxborderw: String,
    pub alpha: String,
}

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Channel {
    #[serde(skip_deserializing)]
    pub id: i64,
    pub name: String,
    pub preview_url: String,
    pub config_path: String,
    pub extra_extensions: String,
    pub service: String,

    #[sqlx(default)]
    #[serde(default)]
    pub utc_offset: i32,
}
