use std::{
    io::{stdin, stdout, Write},
    path::PathBuf,
    process::exit,
};

use clap::Parser;
use log::*;
use rpassword::read_password;
use sqlx::{Pool, Sqlite};

use crate::db::{db_pool, handles::insert_user, models::User};
use crate::utils::config::PlayoutConfig;
use crate::ARGS;

#[derive(Parser, Debug, Clone)]
#[clap(version,
    about = "REST API for ffplayout",
    long_about = None)]
pub struct Args {
    #[clap(short, long, help = "ask for user credentials")]
    pub ask: bool,

    #[clap(long, env, help = "path to database file")]
    pub db: Option<PathBuf>,

    #[clap(
        short,
        long,
        env,
        help = "Run channels by ids immediately (works without webserver and frontend, no listening parameter is needed)",
        num_args = 1..,
    )]
    pub channels: Option<Vec<i32>>,

    #[clap(long, help = "Dump channel configuration to ffplayout_{channel}.toml")]
    pub dump_config: Option<i32>,

    #[clap(
        long,
        help = "import channel configuration from file. Input must be `{channel id} {path to toml}`",
        num_args = 2
    )]
    pub import_config: Option<Vec<String>>,

    #[clap(long, help = "List available channel ids")]
    pub list_channels: bool,

    #[clap(long, env, help = "path to public files")]
    pub public: Option<PathBuf>,

    #[clap(short, env, long, help = "Listen on IP:PORT, like: 127.0.0.1:8787")]
    pub listen: Option<String>,

    #[clap(long, env, help = "Keep log file for given days")]
    pub log_backup_count: Option<usize>,

    #[clap(
        long,
        env,
        help = "Override logging level: trace, debug, info, warn, error"
    )]
    pub log_level: Option<String>,

    #[clap(long, env, help = "Logging path")]
    pub log_path: Option<PathBuf>,

    #[clap(long, env, help = "Log to console")]
    pub log_to_console: bool,

    #[clap(long, env, help = "HLS output path")]
    pub hls_path: Option<PathBuf>,

    #[clap(long, env, help = "Playlist root path")]
    pub playlist_path: Option<PathBuf>,

    #[clap(long, env, help = "Storage root path")]
    pub storage_path: Option<PathBuf>,

    #[clap(short, long, help = "domain name for initialization")]
    pub domain: Option<String>,

    #[clap(short, long, help = "Create admin user")]
    pub username: Option<String>,

    #[clap(short, long, help = "Admin mail address")]
    pub mail: Option<String>,

    #[clap(short, long, help = "Admin password")]
    pub password: Option<String>,
}

pub async fn run_args(pool: &Pool<Sqlite>) -> Result<(), i32> {
    let mut args = ARGS.clone();

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

    if let Some(id) = ARGS.dump_config {
        match PlayoutConfig::dump(&pool, id).await {
            Ok(_) => {
                info!("Dump config to: ffplayout_{id}.toml");
                exit(0);
            }
            Err(e) => {
                error!("Dump config: {e}");

                exit(1);
            }
        };
    }

    if let Some(import) = &ARGS.import_config {
        match PlayoutConfig::import(&pool, import.clone()).await {
            Ok(_) => {
                info!("Import config done...");
                exit(0);
            }
            Err(e) => {
                error!("{e}");

                exit(1);
            }
        };
    }

    Ok(())
}
