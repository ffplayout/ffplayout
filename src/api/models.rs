use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct User {
    pub email: String,
    pub username: String,
    pub password: String,
    pub group_id: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub channel_name: String,
    pub preview_url: String,
    pub settings_path: String,
    pub extra_extensions: String,
}
