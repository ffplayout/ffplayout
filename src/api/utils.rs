use once_cell::sync::OnceCell;
use simplelog::*;

use crate::api::{
    args_parse::Args,
    handles::{db_add_user, db_global, db_init},
    models::User,
};

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
