use regex::Regex;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};

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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub fontsize: String,
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub line_spacing: String,
    pub fontcolor: String,
    pub r#box: String,
    pub boxcolor: String,
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub boxborderw: String,
    #[serde(deserialize_with = "deserialize_number_or_string")]
    pub alpha: String,
}

/// Deserialize number or string
pub fn deserialize_number_or_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrNumberVisitor;

    impl<'de> Visitor<'de> for StringOrNumberVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a number")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            let re = Regex::new(r"0,([0-9]+)").unwrap();
            let clean_string = re.replace_all(value, "0.$1").to_string();
            Ok(clean_string)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
            Ok(value.to_string())
        }
    }

    deserializer.deserialize_any(StringOrNumberVisitor)
}

#[derive(Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Channel {
    #[serde(skip_deserializing)]
    pub id: i32,
    pub name: String,
    pub preview_url: String,
    pub config_path: String,
    pub extra_extensions: String,
    pub service: String,

    #[sqlx(default)]
    #[serde(default)]
    pub utc_offset: i32,
}
