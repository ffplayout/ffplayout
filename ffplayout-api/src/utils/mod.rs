use std::{
    error::Error,
    fs::{self, File},
    io::{stdin, stdout, Write},
    path::Path,
};

use chrono::{format::ParseErrorKind, prelude::*};
use faccess::PathExt;
use once_cell::sync::OnceCell;
use rpassword::read_password;
use serde::{de, Deserialize, Deserializer};
use simplelog::*;
use sqlx::{Pool, Sqlite};

pub mod args_parse;
pub mod channels;
pub mod control;
pub mod errors;
pub mod files;
pub mod playlist;

use crate::db::{
    db_pool,
    handles::{db_init, insert_user, select_channel, select_global},
    models::{Channel, User},
};
use crate::utils::{args_parse::Args, errors::ServiceError};
use ffplayout_lib::utils::{time_to_sec, PlayoutConfig};

#[derive(Clone, Eq, PartialEq)]
pub enum Role {
    Admin,
    User,
    Guest,
}

impl Role {
    pub fn set_role(role: &str) -> Self {
        match role {
            "admin" => Role::Admin,
            "user" => Role::User,
            _ => Role::Guest,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct GlobalSettings {
    pub secret: String,
}

impl GlobalSettings {
    async fn new(conn: &Pool<Sqlite>) -> Self {
        let global_settings = select_global(conn);

        match global_settings.await {
            Ok(g) => g,
            Err(_) => GlobalSettings {
                secret: String::new(),
            },
        }
    }

    pub fn global() -> &'static GlobalSettings {
        INSTANCE.get().expect("Config is not initialized")
    }
}

static INSTANCE: OnceCell<GlobalSettings> = OnceCell::new();

pub async fn init_config(conn: &Pool<Sqlite>) {
    let config = GlobalSettings::new(conn).await;
    INSTANCE.set(config).unwrap();
}

pub fn db_path() -> Result<&'static str, Box<dyn std::error::Error>> {
    let sys_path = Path::new("/usr/share/ffplayout/db");
    let mut db_path = "./ffplayout.db";

    if sys_path.is_dir() && !sys_path.writable() {
        error!("Path {} is not writable!", sys_path.display());
    }

    if sys_path.is_dir() && sys_path.writable() {
        db_path = "/usr/share/ffplayout/db/ffplayout.db";
    } else if Path::new("./assets").is_dir() {
        db_path = "./assets/ffplayout.db";
    }

    if Path::new(db_path).is_file() {
        return Ok(db_path);
    }

    Err(format!("DB path {db_path} not exists!").into())
}

pub async fn run_args(mut args: Args) -> Result<(), i32> {
    if !args.init && args.listen.is_none() && !args.ask && args.username.is_none() {
        error!("Wrong number of arguments! Run ffpapi --help for more information.");

        return Err(0);
    }

    if args.init {
        if let Err(e) = db_init(args.domain).await {
            panic!("{e}");
        };

        return Err(0);
    }

    if args.ask {
        let mut user = String::new();
        print!("Username: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut user)
            .expect("Did not enter a correct name?");
        if let Some('\n') = user.chars().next_back() {
            user.pop();
        }
        if let Some('\r') = user.chars().next_back() {
            user.pop();
        }

        args.username = Some(user);

        print!("Password: ");
        stdout().flush().unwrap();
        let password = read_password();

        args.password = password.ok();

        let mut mail = String::new();
        print!("Mail: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut mail)
            .expect("Did not enter a correct name?");
        if let Some('\n') = mail.chars().next_back() {
            mail.pop();
        }
        if let Some('\r') = mail.chars().next_back() {
            mail.pop();
        }

        args.mail = Some(mail);
    }

    if let Some(username) = args.username {
        if args.mail.is_none() || args.password.is_none() {
            error!("Mail/password missing!");
            return Err(1);
        }

        let user = User {
            id: 0,
            mail: Some(args.mail.unwrap()),
            username: username.clone(),
            password: args.password.unwrap(),
            salt: None,
            role_id: Some(1),
            channel_id: Some(1),
            token: None,
        };

        match db_pool().await {
            Ok(conn) => {
                if let Err(e) = insert_user(&conn, user).await {
                    error!("{e}");
                    return Err(1);
                };
            }

            Err(e) => {
                error!("{e}");
                return Err(1);
            }
        };

        info!("Create admin user \"{username}\" done...");

        return Err(0);
    }

    Ok(())
}

pub fn read_playout_config(path: &str) -> Result<PlayoutConfig, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut config: PlayoutConfig = serde_yaml::from_reader(file)?;

    config.playlist.start_sec = Some(time_to_sec(&config.playlist.day_start));
    config.playlist.length_sec = Some(time_to_sec(&config.playlist.length));

    Ok(config)
}

pub async fn playout_config(
    conn: &Pool<Sqlite>,
    channel_id: &i32,
) -> Result<(PlayoutConfig, Channel), ServiceError> {
    if let Ok(channel) = select_channel(conn, channel_id).await {
        if let Ok(config) = read_playout_config(&channel.config_path.clone()) {
            return Ok((config, channel));
        }
    }

    Err(ServiceError::BadRequest(
        "Error in getting config!".to_string(),
    ))
}

pub async fn read_log_file(
    conn: &Pool<Sqlite>,
    channel_id: &i32,
    date: &str,
) -> Result<String, ServiceError> {
    if let Ok(channel) = select_channel(conn, channel_id).await {
        let mut date_str = "".to_string();

        if !date.is_empty() {
            date_str.push('.');
            date_str.push_str(date);
        }

        if let Ok(config) = read_playout_config(&channel.config_path) {
            let mut log_path = Path::new(&config.logging.path)
                .join("ffplayout.log")
                .display()
                .to_string();
            log_path.push_str(&date_str);

            let file = fs::read_to_string(log_path)?;

            return Ok(file);
        }
    }

    Err(ServiceError::NoContent(
        "Requested log file not exists, or not readable.".to_string(),
    ))
}

pub fn local_utc_offset() -> i32 {
    let mut offset = Local::now().format("%:z").to_string();
    let operator = offset.remove(0);
    let mut utc_offset = 0;

    if let Some((r, f)) = offset.split_once(':') {
        utc_offset = r.parse::<i32>().unwrap_or(0) * 60 + f.parse::<i32>().unwrap_or(0);

        if operator == '-' && utc_offset > 0 {
            utc_offset = -utc_offset;
        }
    }

    utc_offset
}

pub fn naive_date_time_from_str<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    match NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S") {
        Ok(date_time) => Ok(date_time),
        Err(e) => {
            if e.kind() == ParseErrorKind::TooShort {
                NaiveDateTime::parse_from_str(&format!("{s}T00:00:00"), "%Y-%m-%dT%H:%M:%S")
                    .map_err(de::Error::custom)
            } else {
                NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%#z").map_err(de::Error::custom)
            }
        }
    }
}
