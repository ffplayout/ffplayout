use std::{
    io::{stdin, stdout, Write},
    path::{Path, PathBuf},
};

#[cfg(target_family = "unix")]
use std::os::unix::fs::MetadataExt;

use clap::Parser;
use rpassword::read_password;
use sqlx::{Pool, Sqlite};

#[cfg(target_family = "unix")]
use tokio::fs;

use crate::db::{
    handles,
    models::{Channel, User},
};
use crate::utils::{
    advanced_config::AdvancedConfig,
    config::{OutputMode, PlayoutConfig},
    copy_assets,
};
use crate::ARGS;

#[cfg(target_family = "unix")]
use crate::utils::db_path;

#[derive(Parser, Debug, Default, Clone)]
#[clap(version,
    about = "ffplayout - 24/7 broadcasting solution",
    long_about = Some("ffplayout - 24/7 broadcasting solution\n
Stream dynamic playlists or folder contents with the power of ffmpeg.
The target can be an HLS playlist, rtmp/srt/udp server, desktop player
or any other output supported by ffmpeg.\n
ffplayout also provides a web frontend and API to control streaming,
manage config, files, text overlay, etc."),
next_line_help = false,
)]
pub struct Args {
    #[clap(
        short,
        long,
        help_heading = Some("Initial Setup"),
        help = "Initialize defaults: global admin, paths, settings, etc."
    )]
    pub init: bool,

    #[clap(short, long, help_heading = Some("Initial Setup"), help = "Create admin user")]
    pub username: Option<String>,

    #[clap(short, long, help_heading = Some("Initial Setup"), help = "Admin mail address")]
    pub mail: Option<String>,

    #[clap(short, long, help_heading = Some("Initial Setup"), help = "Admin password")]
    pub password: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "Storage root path")]
    pub storage: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "SMTP server for system mails")]
    pub mail_smtp: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "Mail user for system mails")]
    pub mail_user: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "Mail password for system mails")]
    pub mail_password: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "Use TLS for system mails")]
    pub mail_starttls: bool,

    #[clap(long, env, help_heading = Some("Initial Setup / General"), help = "Logging path")]
    pub logs: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup / General"), help = "Path to public files, also HLS playlists")]
    pub public: Option<String>,

    #[clap(long, help_heading = Some("Initial Setup / Playlist"), help = "Path to playlist, or playlist root folder.")]
    pub playlists: Option<String>,

    #[clap(long, help_heading = Some("General"), help = "Add or update a global admin use")]
    pub user_set: bool,

    #[clap(long, env, help_heading = Some("General"), help = "Path to database file")]
    pub db: Option<PathBuf>,

    #[clap(
        long,
        help_heading = Some("General"),
        help = "Drop database. WARNING: this will delete all configurations!"
    )]
    pub drop_db: bool,

    #[clap(
        long,
        help_heading = Some("General"),
        help = "Dump advanced channel configuration to advanced_{channel}.toml"
    )]
    pub dump_advanced: bool,

    #[clap(long, help_heading = Some("General"), help = "Dump channel configuration to ffplayout_{channel}.toml")]
    pub dump_config: bool,

    #[clap(
        long,
        help_heading = Some("General"),
        help = "import advanced channel configuration from file."
    )]
    pub import_advanced: Option<PathBuf>,

    #[clap(long, help_heading = Some("General"), help = "import channel configuration from file.")]
    pub import_config: Option<PathBuf>,

    #[clap(long, help_heading = Some("General"), help = "List available channel ids")]
    pub list_channels: bool,

    #[clap(short, env, long, help_heading = Some("General"), help = "Listen on IP:PORT, like: 127.0.0.1:8787")]
    pub listen: Option<String>,

    #[clap(
        long,
        env,
        help_heading = Some("General"),
        help = "Override logging level: trace, debug, println, warn, eprintln"
    )]
    pub log_level: Option<String>,

    #[clap(long, env, help_heading = Some("General"), help = "Log to console")]
    pub log_to_console: bool,

    #[clap(
        short,
        long,
        env,
        help_heading = Some("General / Playout"),
        help = "Channels by ids to process (for export config, generate playlist, foreground running, etc.)",
        num_args = 1..,
    )]
    pub channel: Option<Vec<i32>>,

    #[clap(long, hide = true, help = "set fake time (for debugging)")]
    pub fake_time: Option<String>,

    #[clap(
        short,
        long,
        help_heading = Some("Playlist"),
        help = "Generate playlist for dates, like: 2022-01-01 - 2022-01-10",
        name = "YYYY-MM-DD",
        num_args = 1..,
    )]
    pub generate: Option<Vec<String>>,

    #[clap(long, help_heading = Some("Playlist"), help = "Optional path list for playlist generations", num_args = 1..)]
    pub paths: Option<Vec<PathBuf>>,

    #[clap(
        short,
        long,
        help_heading = Some("Playlist"),
        help = "Start time in 'hh:mm:ss', 'now' for start with first"
    )]
    pub start: Option<String>,

    #[clap(short = 'T', long, help_heading = Some("Playlist"), help = "JSON template file for generating playlist")]
    pub template: Option<PathBuf>,

    #[clap(long, help_heading = Some("Playlist"), help = "Only validate given playlist")]
    pub validate: bool,

    #[clap(long, env, help_heading = Some("Playout"), help = "Run playout without webserver and frontend")]
    pub foreground: bool,

    #[clap(short, long, help_heading = Some("Playout"), help = "Play folder content")]
    pub folder: Option<PathBuf>,

    #[clap(long, env, help_heading = Some("Playout"), help = "Keep log file for given days")]
    pub log_backup_count: Option<usize>,

    #[clap(long, env, help_heading = Some("Playout"), help = "Add timestamp to log line")]
    pub log_timestamp: bool,

    #[clap(short, long, help_heading = Some("Playout"), help = "Set output mode: desktop, hls, null, stream")]
    pub output: Option<OutputMode>,

    #[clap(short, long, help_heading = Some("Playout"), help = "Set audio volume")]
    pub volume: Option<f64>,

    #[clap(long, help_heading = Some("Playout"), help = "Skip validation process")]
    pub skip_validation: bool,
}

fn global_user(args: &mut Args) {
    let mut user = String::new();
    let mut mail = String::new();

    if args.username.is_none() {
        print!("Global admin: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut user)
            .expect("Did not enter a correct name?");

        args.username = Some(user.trim().to_string());
    }

    if args.password.is_none() {
        print!("Password: ");
        stdout().flush().unwrap();
        let password = read_password();

        args.password = password.ok();
    }

    if args.mail.is_none() {
        print!("Mail: ");
        stdout().flush().unwrap();

        stdin()
            .read_line(&mut mail)
            .expect("Did not enter a correct name?");

        args.mail = Some(mail.trim().to_string());
    }
}

pub async fn run_args(pool: &Pool<Sqlite>) -> Result<(), i32> {
    let mut args = ARGS.clone();

    if !args.dump_advanced && !args.dump_config && !args.drop_db {
        if let Err(e) = handles::db_migrate(pool).await {
            panic!("{e}");
        };
    }

    let channels = handles::select_related_channels(pool, None)
        .await
        .unwrap_or(vec![Channel::default()]);

    let mut error_code = -1;

    if args.init {
        let check_user = handles::select_users(pool).await;

        let mut storage = String::new();
        let mut playlist = String::new();
        let mut logging = String::new();
        let mut public = String::new();
        let mut mail_smtp = String::new();
        let mut mail_user = String::new();
        let mut mail_starttls = String::new();
        let mut global = handles::select_global(pool).await.map_err(|_| 1)?;

        if check_user.unwrap_or_default().is_empty() {
            global_user(&mut args);
        }

        if let Some(st) = args.storage {
            global.storage = st;
        } else {
            print!("Storage path [{}]: ", global.storage);
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut storage)
                .expect("Did not enter a correct path?");

            if !storage.trim().is_empty() {
                global.storage = storage
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            }
        }

        if let Some(pl) = args.playlists {
            global.playlists = pl
        } else {
            print!("Playlist path [{}]: ", global.playlists);
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut playlist)
                .expect("Did not enter a correct path?");

            if !playlist.trim().is_empty() {
                global.playlists = playlist
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            }
        }

        if let Some(lp) = args.logs {
            global.logs = lp;
        } else {
            print!("Logging path [{}]: ", global.logs);
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut logging)
                .expect("Did not enter a correct path?");

            if !logging.trim().is_empty() {
                global.logs = logging
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            }
        }

        if let Some(p) = args.public {
            global.public = p;
        } else {
            print!("Public (HLS) path [{}]: ", global.public);
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut public)
                .expect("Did not enter a correct path?");

            if !public.trim().is_empty() {
                global.public = public
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            }
        }

        if let Some(smtp) = args.mail_smtp {
            global.mail_smtp = smtp;
        } else {
            print!("SMTP server [{}]: ", global.mail_smtp);
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut mail_smtp)
                .expect("Did not enter a correct SMTP server?");

            if !mail_smtp.trim().is_empty() {
                global.mail_smtp = mail_smtp
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            }
        }

        if let Some(user) = args.mail_user {
            global.mail_user = user;
        } else {
            print!("SMTP user [{}]: ", global.mail_user);
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut mail_user)
                .expect("Did not enter a correct SMTP user?");

            if !mail_user.trim().is_empty() {
                global.mail_user = mail_user
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            }
        }

        if let Some(pass) = args.mail_password {
            global.mail_password = pass;
        } else {
            print!(
                "SMTP password [{}]: ",
                if global.mail_password.is_empty() {
                    ""
                } else {
                    "********"
                }
            );
            stdout().flush().unwrap();
            let password = read_password().unwrap_or_default();

            if !password.trim().is_empty() {
                global.mail_password = password.trim().to_string();
            }
        }

        if args.mail_starttls {
            global.mail_starttls = true;
        } else {
            print!(
                "SMTP use TLS [{}]: ",
                if global.mail_starttls { "yes" } else { "no" }
            );
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut mail_starttls)
                .expect("Did not enter a yes or no?");

            if !mail_starttls.trim().is_empty() {
                global.mail_starttls = mail_starttls.trim().to_lowercase().starts_with('y');
            }
        }

        if let Err(e) = handles::update_global(pool, global.clone()).await {
            eprintln!("{e}");
            error_code = 1;
        };

        let mut channel = handles::select_channel(pool, &1).await.unwrap();
        channel.public = global.public;
        channel.playlists = global.playlists;
        channel.storage = global.storage;

        let mut storage_path = PathBuf::from(channel.storage.clone());

        if global.shared {
            storage_path = storage_path.join("1");

            channel.public = Path::new(&channel.public)
                .join("1")
                .to_string_lossy()
                .to_string();
            channel.playlists = Path::new(&channel.playlists)
                .join("1")
                .to_string_lossy()
                .to_string();
            channel.storage = storage_path.to_string_lossy().to_string();
        };

        if let Err(e) = copy_assets(&storage_path).await {
            eprintln!("{e}");
        };

        handles::update_channel(pool, 1, channel).await.unwrap();

        #[cfg(target_family = "unix")]
        {
            update_permissions().await;
        }

        println!("\nSet global settings done...");
    } else if args.user_set {
        global_user(&mut args);
    }

    if let Some(username) = args.username {
        error_code = 0;

        let chl: Vec<i32> = channels.clone().iter().map(|c| c.id).collect();

        let ff_user = User {
            id: 0,
            mail: Some(args.mail.unwrap()),
            username: username.clone(),
            password: args.password.unwrap(),
            role_id: Some(1),
            channel_ids: Some(chl.clone()),
            token: None,
        };

        if let Err(e) = handles::insert_or_update_user(pool, ff_user).await {
            eprintln!("{e}");
            error_code = 1;
        };

        println!("Create/update global admin user \"{username}\" done...");
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

    if ARGS.dump_advanced {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                match AdvancedConfig::dump(pool, *id).await {
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
        } else {
            eprintln!("Channel ID(s) needed! Use `--channel 1 ...`");
            error_code = 1;
        }
    }

    if ARGS.dump_config {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                match PlayoutConfig::dump(pool, *id).await {
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
        } else {
            eprintln!("Channel ID(s) needed! Use `--channel 1 ...`");
            error_code = 1;
        }
    }

    if let Some(path) = &ARGS.import_advanced {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                match AdvancedConfig::import(pool, *id, path).await {
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
        } else {
            eprintln!("Channel ID(s) needed! Use `--channel 1 ...`");
            error_code = 1;
        }
    }

    if let Some(path) = &ARGS.import_config {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                match PlayoutConfig::import(pool, *id, path).await {
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
        } else {
            eprintln!("Channel ID(s) needed! Use `--channel 1 ...`");
            error_code = 1;
        }
    }

    if error_code > -1 {
        Err(error_code)
    } else {
        Ok(())
    }
}

#[cfg(target_family = "unix")]
async fn update_permissions() {
    let db_path = Path::new(db_path().unwrap());
    let uid = nix::unistd::Uid::current();
    let parent_owner = db_path.parent().unwrap().metadata().unwrap().uid();
    let user = nix::unistd::User::from_uid(parent_owner.into())
        .unwrap_or_default()
        .unwrap();

    if uid.is_root() && uid.to_string() != parent_owner.to_string() {
        println!("Adjust DB permission...");

        let db = fs::canonicalize(db_path).await.unwrap();
        let shm = fs::canonicalize(db_path.with_extension("db-shm"))
            .await
            .unwrap();
        let wal = fs::canonicalize(db_path.with_extension("db-wal"))
            .await
            .unwrap();

        nix::unistd::chown(&db, Some(user.uid), Some(user.gid)).expect("Change DB owner");

        if shm.is_file() {
            nix::unistd::chown(&shm, Some(user.uid), Some(user.gid)).expect("Change DB-SHM owner");
        }

        if wal.is_file() {
            nix::unistd::chown(&wal, Some(user.uid), Some(user.gid)).expect("Change DB-WAL owner");
        }
    }
}
