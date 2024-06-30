use std::{
    io::{stdin, stdout, Write},
    path::PathBuf,
    process::exit,
};

use clap::Parser;
use rpassword::read_password;
use sqlx::{Pool, Sqlite};

use crate::db::{
    handles::{self, insert_user},
    models::{Channel, GlobalSettings, User},
};
use crate::utils::{
    advanced_config::AdvancedConfig,
    config::{OutputMode, PlayoutConfig},
};
use crate::ARGS;

#[derive(Parser, Debug, Clone)]
#[clap(version,
    about = "ffplayout - 24/7 broadcasting solution",
    long_about = None)]
pub struct Args {
    #[clap(
        short,
        long,
        help = "Initialize defaults: global admin, paths, settings, etc."
    )]
    pub init: bool,

    #[clap(short, long, help = "Add a global admin user")]
    pub add: bool,

    #[clap(long, env, help = "path to database file")]
    pub db: Option<PathBuf>,

    #[clap(
        short,
        long,
        env,
        help = "Channels by ids to process (for foreground, etc.)",
        num_args = 1..,
    )]
    pub channels: Option<Vec<i32>>,

    #[clap(long, env, help = "Run playout without webserver and frontend.")]
    pub foreground: bool,

    #[clap(
        long,
        help = "Dump advanced channel configuration to advanced_{channel}.toml"
    )]
    pub dump_advanced: Option<i32>,

    #[clap(long, help = "Dump channel configuration to ffplayout_{channel}.toml")]
    pub dump_config: Option<i32>,

    #[clap(
        long,
        help = "import advanced channel configuration from file. Input must be `{channel id} {path to toml}`",
        num_args = 2
    )]
    pub import_advanced: Option<Vec<String>>,

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

    #[clap(short, long, help = "Play folder content")]
    pub folder: Option<PathBuf>,

    #[clap(
        short,
        long,
        help = "Generate playlist for dates, like: 2022-01-01 - 2022-01-10",
        name = "YYYY-MM-DD",
        num_args = 1..,
    )]
    pub generate: Option<Vec<String>>,

    #[clap(long, help = "Optional folder path list for playlist generations", num_args = 1..)]
    pub gen_paths: Option<Vec<PathBuf>>,

    #[clap(long, env, help = "Keep log file for given days")]
    pub log_backup_count: Option<usize>,

    #[clap(
        long,
        env,
        help = "Override logging level: trace, debug, println, warn, eprintln"
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

    #[clap(long, env, help = "Share storage across channels")]
    pub shared_storage: bool,

    #[clap(short, long, help = "Create admin user")]
    pub username: Option<String>,

    #[clap(short, long, help = "Admin mail address")]
    pub mail: Option<String>,

    #[clap(short, long, help = "Admin password")]
    pub password: Option<String>,

    #[clap(long, help = "Path to playlist, or playlist root folder.")]
    pub playlist: Option<PathBuf>,

    #[clap(
        short,
        long,
        help = "Start time in 'hh:mm:ss', 'now' for start with first"
    )]
    pub start: Option<String>,

    #[clap(short = 'T', long, help = "JSON Template file for generating playlist")]
    pub template: Option<PathBuf>,

    #[clap(short, long, help = "Set output mode: desktop, hls, null, stream")]
    pub output: Option<OutputMode>,

    #[clap(short, long, help = "Set audio volume")]
    pub volume: Option<f64>,

    #[clap(long, help = "Skip validation process")]
    pub skip_validation: bool,

    #[clap(long, help = "Only validate given playlist")]
    pub validate: bool,
}

fn global_user(args: &mut Args) {
    let mut user = String::new();
    let mut mail = String::new();

    print!("Global admin: ");
    stdout().flush().unwrap();

    stdin()
        .read_line(&mut user)
        .expect("Did not enter a correct name?");

    args.username = Some(user.trim().to_string());

    print!("Password: ");
    stdout().flush().unwrap();
    let password = read_password();

    args.password = password.ok();

    print!("Mail: ");
    stdout().flush().unwrap();

    stdin()
        .read_line(&mut mail)
        .expect("Did not enter a correct name?");

    args.mail = Some(mail.trim().to_string());
}

pub async fn run_args(pool: &Pool<Sqlite>) -> Result<(), i32> {
    let channels = handles::select_related_channels(pool, None).await;
    let mut args = ARGS.clone();

    if args.init {
        let check_user = handles::select_users(pool).await;

        let mut storage = String::new();
        let mut playlist = String::new();
        let mut logging = String::new();
        let mut hls = String::new();
        let mut shared_store = String::new();
        let mut global = GlobalSettings {
            id: 0,
            secret: None,
            hls_path: String::new(),
            playlist_path: String::new(),
            storage_path: String::new(),
            logging_path: String::new(),
            shared_storage: false,
        };

        if check_user.unwrap_or_default().is_empty() {
            global_user(&mut args);
        }

        print!("Storage path [/var/lib/ffplayout/tv-media]: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut storage)
            .expect("Did not enter a correct path?");

        if storage.trim().is_empty() {
            global.storage_path = "/var/lib/ffplayout/tv-media".to_string();
        } else {
            global.storage_path = storage
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
        }

        print!("Playlist path [/var/lib/ffplayout/playlists]: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut playlist)
            .expect("Did not enter a correct path?");

        if playlist.trim().is_empty() {
            global.playlist_path = "/var/lib/ffplayout/playlists".to_string();
        } else {
            global.playlist_path = playlist
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
        }

        print!("Logging path [/var/log/ffplayout]: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut logging)
            .expect("Did not enter a correct path?");

        if logging.trim().is_empty() {
            global.logging_path = "/var/log/ffplayout".to_string();
        } else {
            global.logging_path = logging
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
        }

        print!("HLS path [/usr/share/ffplayout/public]: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut hls)
            .expect("Did not enter a correct path?");

        if hls.trim().is_empty() {
            global.hls_path = "/usr/share/ffplayout/public".to_string();
        } else {
            global.hls_path = hls
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
        }

        print!("Shared storage [Y/n]: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut shared_store)
            .expect("Did not enter a correct path?");

        global.shared_storage = shared_store.trim().to_lowercase().starts_with('y');

        if let Err(e) = handles::update_global(pool, global.clone()).await {
            eprintln!("{e}");
            return Err(1);
        };

        if !global.shared_storage {
            let mut channel = handles::select_channel(pool, &1).await.unwrap();
            channel.preview_url = "http://127.0.0.1:8787/1/stream.m3u8".to_string();

            handles::update_channel(pool, 1, channel).await.unwrap();
        };

        println!("Set global settings...");
    }

    if args.add {
        global_user(&mut args);
    }

    if let Some(username) = args.username {
        if args.mail.is_none() || args.password.is_none() {
            eprintln!("Mail/password missing!");
            return Err(1);
        }

        let user = User {
            id: 0,
            mail: Some(args.mail.unwrap()),
            username: username.clone(),
            password: args.password.unwrap(),
            role_id: Some(1),
            channel_ids: Some(
                channels
                    .unwrap_or(vec![Channel::default()])
                    .iter()
                    .map(|c| c.id)
                    .collect(),
            ),
            token: None,
        };

        if let Err(e) = insert_user(pool, user).await {
            eprintln!("{e}");
            return Err(1);
        };

        println!("Create global admin user \"{username}\" done...");

        return Err(0);
    }

    if ARGS.list_channels {
        match channels {
            Ok(channels) => {
                let chl = channels
                    .iter()
                    .map(|c| (c.id, c.name.clone()))
                    .collect::<Vec<(i32, String)>>();

                println!(
                    "Available channels:\n{}",
                    chl.iter()
                        .map(|(i, t)| format!("    {i}: '{t}'"))
                        .collect::<Vec<String>>()
                        .join("\n")
                );

                return Err(0);
            }
            Err(e) => {
                eprintln!("List channels: {e}");

                exit(1);
            }
        }
    }

    if let Some(id) = ARGS.dump_config {
        match PlayoutConfig::dump(pool, id).await {
            Ok(_) => {
                println!("Dump config to: ffplayout_{id}.toml");
                exit(0);
            }
            Err(e) => {
                eprintln!("Dump config: {e}");

                exit(1);
            }
        };
    }

    if let Some(id) = ARGS.dump_config {
        match PlayoutConfig::dump(pool, id).await {
            Ok(_) => {
                println!("Dump config to: ffplayout_{id}.toml");
                exit(0);
            }
            Err(e) => {
                eprintln!("Dump config: {e}");

                exit(1);
            }
        };
    }

    if let Some(id) = ARGS.dump_advanced {
        match AdvancedConfig::dump(pool, id).await {
            Ok(_) => {
                println!("Dump config to: advanced_{id}.toml");
                exit(0);
            }
            Err(e) => {
                eprintln!("Dump config: {e}");

                exit(1);
            }
        };
    }

    if let Some(import) = &ARGS.import_config {
        match PlayoutConfig::import(pool, import.clone()).await {
            Ok(_) => {
                println!("Import config done...");
                exit(0);
            }
            Err(e) => {
                eprintln!("{e}");

                exit(1);
            }
        };
    }

    if let Some(import) = &ARGS.import_advanced {
        match AdvancedConfig::import(pool, import.clone()).await {
            Ok(_) => {
                println!("Import config done...");
                exit(0);
            }
            Err(e) => {
                eprintln!("{e}");

                exit(1);
            }
        };
    }

    Ok(())
}
