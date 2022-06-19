use std::{error::Error, fs::File, path::Path};

use faccess::PathExt;
use once_cell::sync::OnceCell;
use simplelog::*;

use crate::api::{
    args_parse::Args,
    errors::ServiceError,
    handles::{db_add_user, db_get_settings, db_global, db_init},
    models::User,
};
use crate::utils::PlayoutConfig;

#[derive(PartialEq, Clone)]
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
    async fn new() -> Self {
        let global_settings = db_global();

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

pub async fn init_config() {
    let config = GlobalSettings::new().await;
    INSTANCE.set(config).unwrap();
}

pub fn db_path() -> Result<String, Box<dyn std::error::Error>> {
    let sys_path = Path::new("/usr/share/ffplayout");
    let mut db_path = String::from("./ffplayout.db");

    if sys_path.is_dir() && sys_path.writable() {
        db_path = String::from("/usr/share/ffplayout/ffplayout.db");
    } else if Path::new("./assets").is_dir() {
        db_path = String::from("./assets/ffplayout.db");
    }

    Ok(db_path)
}

pub async fn run_args(args: Args) -> Result<(), i32> {
    if !args.init && args.listen.is_none() && args.username.is_none() {
        error!("Wrong number of arguments! Run ffpapi --help for more information.");

        return Err(0);
    }

    if args.init {
        if let Err(e) = db_init().await {
            panic!("{e}");
        };

        return Err(0);
    }

    if let Some(username) = args.username {
        if args.email.is_none() || args.password.is_none() {
            error!("Email/password missing!");
            return Err(1);
        }

        let user = User {
            id: 0,
            email: Some(args.email.unwrap()),
            username: username.clone(),
            password: args.password.unwrap(),
            salt: None,
            role_id: Some(1),
            token: None,
        };

        if let Err(e) = db_add_user(user).await {
            error!("{e}");
            return Err(1);
        };

        info!("Create admin user \"{username}\" done...");

        return Err(0);
    }

    Ok(())
}

pub fn read_playout_config(path: &str) -> Result<PlayoutConfig, Box<dyn Error>> {
    let file = File::open(path)?;
    let config: PlayoutConfig = serde_yaml::from_reader(file)?;

    Ok(config)
}

pub async fn playout_config(channel_id: &i64) -> Result<PlayoutConfig, ServiceError> {
    if let Ok(settings) = db_get_settings(channel_id).await {
        if let Ok(config) = read_playout_config(&settings.config_path) {
            return Ok(config);
        }
    }

    Err(ServiceError::BadRequest(
        "Error in getting config!".to_string(),
    ))
}
