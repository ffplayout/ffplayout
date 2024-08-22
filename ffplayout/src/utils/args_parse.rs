use std::{
    io::{stdin, stdout, Write},
    path::{Path, PathBuf},
};

#[cfg(target_family = "unix")]
use std::{fs, process::exit};

use clap::Parser;
use rpassword::read_password;
use sqlx::{Pool, Sqlite};

use crate::db::{
    handles,
    models::{Channel, GlobalSettings, User},
};
use crate::utils::{
    advanced_config::AdvancedConfig,
    config::{OutputMode, PlayoutConfig},
};
use crate::ARGS;

#[cfg(target_family = "unix")]
use crate::utils::db_path;

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

    #[clap(long, env, help = "Path to database file")]
    pub db: Option<PathBuf>,

    #[clap(
        long,
        env,
        help = "Drop database. WARNING: this will delete all configurations!"
    )]
    pub drop_db: bool,

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

    #[clap(long, env, help = "Public (HLS) output path")]
    pub public_root: Option<String>,

    #[clap(long, env, help = "Playlist root path")]
    pub playlist_root: Option<String>,

    #[clap(long, env, help = "Storage root path")]
    pub storage_root: Option<String>,

    #[clap(
        long,
        env,
        help = "Share storage root across channels, important for running in Container"
    )]
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
    let mut args = ARGS.clone();

    if args.dump_advanced.is_none() && args.dump_config.is_none() {
        if let Err(e) = handles::db_migrate(pool).await {
            panic!("{e}");
        };
    }

    let channels = handles::select_related_channels(pool, None)
        .await
        .unwrap_or(vec![Channel::default()]);

    let mut error_code = -1;

    if args.init {
        #[cfg(target_family = "unix")]
        let process_user = nix::unistd::User::from_name("ffpu").unwrap_or_default();

        #[cfg(target_family = "unix")]
        let mut fix_permission = false;

        #[cfg(target_family = "unix")]
        {
            let uid = nix::unistd::Uid::current();
            let current_user = nix::unistd::User::from_uid(uid).unwrap_or_default();

            if current_user != process_user {
                let user_name = current_user.unwrap().name;
                let mut fix_perm = String::new();

                println!(
                    "\nYou run the initialization as user {}.\nFix permissions after initialization?\n",
                    user_name
                );

                print!("Fix permission [Y/n]: ");
                stdout().flush().unwrap();

                stdin()
                    .read_line(&mut fix_perm)
                    .expect("Did not enter a yes or no?");

                fix_permission = fix_perm.trim().to_lowercase().starts_with('y');

                if fix_permission && user_name != "root" {
                    println!("\nYou do not have permission to change DB file ownership!\nRun as proper process user or root.");

                    exit(1);
                }
            }
        }

        let check_user = handles::select_users(pool).await;

        let mut storage = String::new();
        let mut playlist = String::new();
        let mut logging = String::new();
        let mut hls = String::new();
        let mut shared_store = String::new();
        let mut global = GlobalSettings {
            id: 0,
            secret: None,
            logging_path: String::new(),
            playlist_root: String::new(),
            public_root: String::new(),
            storage_root: String::new(),
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
            global.storage_root = "/var/lib/ffplayout/tv-media".to_string();
        } else {
            global.storage_root = storage
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
            global.playlist_root = "/var/lib/ffplayout/playlists".to_string();
        } else {
            global.playlist_root = playlist
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

        print!("Public (HLS) path [/usr/share/ffplayout/public]: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut hls)
            .expect("Did not enter a correct path?");

        if hls.trim().is_empty() {
            global.public_root = "/usr/share/ffplayout/public".to_string();
        } else {
            global.public_root = hls
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
        }

        print!("Shared storage [Y/n]: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut shared_store)
            .expect("Did not enter a yes or no?");

        global.shared_storage = shared_store.trim().to_lowercase().starts_with('y');

        if let Err(e) = handles::update_global(pool, global.clone()).await {
            eprintln!("{e}");
            error_code = 1;
        };

        let mut channel = handles::select_channel(pool, &1).await.unwrap();
        channel.hls_path = global.public_root;
        channel.playlist_path = global.playlist_root;
        channel.storage_path = global.storage_root;

        if global.shared_storage {
            channel.preview_url = "http://127.0.0.1:8787/1/stream.m3u8".to_string();
            channel.hls_path = Path::new(&channel.hls_path)
                .join("1")
                .to_string_lossy()
                .to_string();
            channel.playlist_path = Path::new(&channel.playlist_path)
                .join("1")
                .to_string_lossy()
                .to_string();
            channel.storage_path = Path::new(&channel.storage_path)
                .join("1")
                .to_string_lossy()
                .to_string();
        };

        handles::update_channel(pool, 1, channel).await.unwrap();

        #[cfg(target_family = "unix")]
        if fix_permission {
            let db_path = Path::new(db_path().unwrap()).with_extension("");
            let user = process_user.unwrap();

            let db = fs::canonicalize(db_path.with_extension("db")).unwrap();
            let shm = fs::canonicalize(db_path.with_extension("db-shm")).unwrap();
            let wal = fs::canonicalize(db_path.with_extension("db-wal")).unwrap();

            nix::unistd::chown(&db, Some(user.uid), Some(user.gid)).expect("Change DB owner");

            if shm.is_file() {
                nix::unistd::chown(&shm, Some(user.uid), Some(user.gid))
                    .expect("Change DB-SHM owner");
            }

            if wal.is_file() {
                nix::unistd::chown(&wal, Some(user.uid), Some(user.gid))
                    .expect("Change DB-WAL owner");
            }
        }

        println!("\nSet global settings done...");
    }

    if args.add {
        global_user(&mut args);
    }

    if let Some(username) = args.username {
        error_code = 0;

        if args.mail.is_none() || args.password.is_none() {
            eprintln!("Mail/password missing!");
            error_code = 1;
        }

        let chl: Vec<i32> = channels.clone().iter().map(|c| c.id).collect();

        let user = User {
            id: 0,
            mail: Some(args.mail.unwrap()),
            username: username.clone(),
            password: args.password.unwrap(),
            role_id: Some(1),
            channel_ids: Some(chl.clone()),
            token: None,
        };

        if let Err(e) = handles::insert_user(pool, user).await {
            eprintln!("{e}");
            error_code = 1;
        };

        println!("Create global admin user \"{username}\" done...");
    }

    if !args.init
        && args.storage_root.is_some()
        && args.playlist_root.is_some()
        && args.public_root.is_some()
        && args.log_path.is_some()
    {
        error_code = 0;

        let global = GlobalSettings {
            id: 0,
            secret: None,
            logging_path: args.log_path.unwrap().to_string_lossy().to_string(),
            playlist_root: args.playlist_root.unwrap(),
            public_root: args.public_root.unwrap(),
            storage_root: args.storage_root.unwrap(),
            shared_storage: args.shared_storage,
        };

        match handles::update_global(pool, global.clone()).await {
            Ok(_) => println!("Update global paths..."),
            Err(e) => {
                eprintln!("{e}");
                error_code = 1;
            }
        };
    }

    if ARGS.list_channels {
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

        error_code = 0;
    }

    if let Some(id) = ARGS.dump_config {
        match PlayoutConfig::dump(pool, id).await {
            Ok(_) => {
                println!("Dump config to: ffplayout_{id}.toml");
                error_code = 0;
            }
            Err(e) => {
                eprintln!("Dump config: {e}");
                error_code = 1;
            }
        };
    }

    if let Some(id) = ARGS.dump_config {
        match PlayoutConfig::dump(pool, id).await {
            Ok(_) => {
                println!("Dump config to: ffplayout_{id}.toml");
                error_code = 0;
            }
            Err(e) => {
                eprintln!("Dump config: {e}");
                error_code = 1;
            }
        };
    }

    if let Some(id) = ARGS.dump_advanced {
        match AdvancedConfig::dump(pool, id).await {
            Ok(_) => {
                println!("Dump config to: advanced_{id}.toml");
                error_code = 0;
            }
            Err(e) => {
                eprintln!("Dump config: {e}");
                error_code = 1;
            }
        };
    }

    if let Some(import) = &ARGS.import_config {
        match PlayoutConfig::import(pool, import.clone()).await {
            Ok(_) => {
                println!("Import config done...");
                error_code = 0;
            }
            Err(e) => {
                eprintln!("{e}");
                error_code = 1;
            }
        };
    }

    if let Some(import) = &ARGS.import_advanced {
        match AdvancedConfig::import(pool, import.clone()).await {
            Ok(_) => {
                println!("Import config done...");
                error_code = 0;
            }
            Err(e) => {
                eprintln!("{e}");
                error_code = 1;
            }
        };
    }

    if error_code > -1 {
        Err(error_code)
    } else {
        Ok(())
    }
}
