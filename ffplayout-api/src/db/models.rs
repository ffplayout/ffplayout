use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteRow, FromRow, Row};

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i32,
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
    pub role_id: Option<i32>,
    #[sqlx(default)]
    #[serde(skip_serializing)]
    pub channel_id: Option<i32>,
    #[sqlx(default)]
    pub token: Option<String>,
}

fn empty_string() -> String {
    "".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoginUser {
    pub id: i32,
    pub username: String,
}

impl LoginUser {
    pub fn new(id: i32, username: String) -> Self {
        Self { id, username }
    }
}
#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow)]
pub struct TextPreset {
    #[sqlx(default)]
    #[serde(skip_deserializing)]
    pub id: i32,
    pub channel_id: i32,
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

#[derive(Debug, Deserialize, Serialize, sqlx::Type)]
pub struct Channel {
    #[serde(skip_deserializing)]
    pub id: i32,
    pub name: String,
    pub preview_url: String,
    pub config_path: String,
    pub extra_extensions: Vec<String>,
    pub service: String,

    #[serde(default)]
    pub utc_offset: i32,
}

impl FromRow<'_, SqliteRow> for Channel {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.get("id"),
            name: row.get("name"),
            preview_url: row.get("preview_url"),
            config_path: row.get("config_path"),
            extra_extensions: row
                .get::<String, &str>("extra_extensions")
                .split(',')
                .map(|s| s.to_string())
                .collect(),
            service: row.get("service"),
            utc_offset: 0,
        })
    }
}
