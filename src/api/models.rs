use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Option<i64>,
    #[sqlx(default)]
    pub email: Option<String>,
    pub username: String,
    #[sqlx(default)]
    pub password: String,
    #[sqlx(default)]
    pub group_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub id: i64,
    pub channel_name: String,
    pub preview_url: String,
    pub settings_path: String,
    pub extra_extensions: String,
}
