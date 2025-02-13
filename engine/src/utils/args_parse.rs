use std::path::{Path, PathBuf};

#[cfg(target_family = "unix")]
use std::os::unix::fs::MetadataExt;

use clap::Parser;
use inquire::{Confirm, CustomType, Password, PasswordDisplayMode, Text};
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
};
use crate::ARGS;

use super::errors::ProcessError;

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
    pub smtp_server: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "SMTP user for system mails")]
    pub smtp_user: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "SMTP password for system mails")]
    pub smtp_password: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "Use TLS for system SMTP", value_name = "TRUE/FALSE")]
    pub smtp_starttls: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup"), help = "SMTP port for system mail")]
    pub smtp_port: Option<u16>,

    #[clap(long, env, help_heading = Some("Initial Setup / General"), help = "Logging path")]
    pub logs: Option<String>,

    #[clap(long, env, help_heading = Some("Initial Setup / General"), help = "Path to public files, also HLS playlists")]
    pub public: Option<String>,

    #[clap(long, help_heading = Some("Initial Setup / Playlist"), help = "Path to playlist, or playlist root folder")]
    pub playlists: Option<String>,

    #[clap(long, help_heading = Some("General"), help = "Add or update a global admin user")]
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

    #[clap(long, help_heading = Some("General"), help = "Import channel configuration from file")]
    pub import_config: Option<PathBuf>,

    #[clap(long, help_heading = Some("General"), help = "List available channel ids")]
    pub list_channels: bool,

    #[clap(short, env, long, help_heading = Some("General"), help = "Listen on IP:PORT, like: 127.0.0.1:8787")]
    pub listen: Option<String>,

    #[clap(
        long,
        env,
        help_heading = Some("General"),
        help = "Override logging level: trace, debug, info, warn, error"
    )]
    pub log_level: Option<String>,

    #[clap(long, env, help_heading = Some("General"), help = "Log to console")]
    pub log_to_console: bool,

    #[clap(long, env, help_heading = Some("General"), help = "Keep log file for given days")]
    pub log_backup_count: Option<usize>,

    #[clap(long, env, help_heading = Some("General"), help = "Add timestamp to log line")]
    pub log_timestamp: bool,

    #[clap(
        short,
        long,
        env,
        help_heading = Some("General / Playout"),
        help = "Channels by ids to process (for export config, generate playlist, foreground running, etc.)",
        num_args = 1..,
    )]
    pub channel: Option<Vec<i32>>,

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

    #[clap(short, long, help_heading = Some("Playout"), help = "Set output mode: desktop, hls, null, stream")]
    pub output: Option<OutputMode>,

    #[clap(short, long, help_heading = Some("Playout"), help = "Set audio volume")]
    pub volume: Option<f64>,

    #[clap(long, help_heading = Some("Playout"), help = "Skip validation process")]
    pub skip_validation: bool,

    #[clap(long, hide = true, help = "Set fake time (for debugging)")]
    pub fake_time: Option<String>,

    #[clap(long, hide = true, help = "Send a test email (for debugging)")]
    pub test_mail: bool,
}

fn global_user(args: &mut Args) {
    if args.username.is_none() {
        args.username = Text::new("Username:").prompt().ok();
    }

    if args.password.is_none() {
        args.password = Password::new("Password:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .prompt()
            .ok();
    }

    if args.mail.is_none() {
        args.mail = Text::new("Email:").prompt().ok();
    }
}

fn clean_input(input: &str) -> String {
    input
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string()
}

pub async fn init_args(pool: &Pool<Sqlite>) -> Result<bool, ProcessError> {
    let mut args = ARGS.clone();
    let mut init = false;

    if !args.dump_advanced && !args.dump_config && !args.drop_db {
        init = handles::db_migrate(pool).await?;
    }

    let channels = handles::select_related_channels(pool, None)
        .await
        .unwrap_or(vec![Channel::default()]);

    if args.init {
        let check_user = handles::select_users(pool).await;
        let mut global = handles::select_global(pool).await?;

        if check_user.unwrap_or_default().is_empty() {
            global_user(&mut args);
        }

        if let Some(st) = args.storage {
            global.storage = st;
        } else {
            global.storage = Text::new("Storage path:")
                .with_default(&global.storage)
                .with_formatter(&clean_input)
                .prompt()?;
        }

        if let Some(pl) = args.playlists {
            global.playlists = pl;
        } else {
            global.playlists = Text::new("Playlist path:")
                .with_default(&global.playlists)
                .with_formatter(&clean_input)
                .prompt()?;
        }

        if let Some(lp) = args.logs {
            global.logs = lp;
        } else {
            global.logs = Text::new("Logging path:")
                .with_default(&global.logs)
                .with_formatter(&clean_input)
                .prompt()?;
        }

        if let Some(p) = args.public {
            global.public = p;
        } else {
            global.public = Text::new("Public (HLS) path:")
                .with_default(&global.public)
                .with_formatter(&clean_input)
                .prompt()?;
        }

        if let Some(smtp) = args.smtp_server {
            global.smtp_server = smtp;
        } else {
            global.smtp_server = Text::new("SMTP server:")
                .with_default(&global.smtp_server)
                .with_formatter(&clean_input)
                .prompt()?;
        }

        if let Some(user) = args.smtp_user {
            global.smtp_user = user;
        } else {
            global.smtp_user = Text::new("SMTP user:")
                .with_default(&global.smtp_user)
                .with_formatter(&clean_input)
                .prompt()?;
        }

        if let Some(pass) = args.smtp_password {
            global.smtp_password = pass;
        } else {
            let pass = Password::new("SMTP password:")
                .with_help_message("Hit enter to use existing one")
                .with_display_mode(PasswordDisplayMode::Masked)
                .prompt()?;

            if !pass.is_empty() {
                global.smtp_password = pass;
            }
        }

        match args.smtp_starttls {
            Some(val) => match val.to_lowercase().as_str() {
                "true" => global.smtp_starttls = true,
                "false" => global.smtp_starttls = false,
                _ => {
                    return Err(ProcessError::Input(
                        "--smtp-starttls accept true or false".to_string(),
                    ))
                }
            },
            None => {
                global.smtp_starttls = Confirm::new("SMTP use TLS").with_default(false).prompt()?;
            }
        }

        // if args.smtp_starttls {
        //     global.smtp_starttls = true;
        // } else {
        //     global.smtp_starttls = Confirm::new("SMTP use TLS").with_default(false).prompt()?;
        // }

        if let Some(port) = args.smtp_port {
            global.smtp_port = port;
        } else {
            global.smtp_port = CustomType::<u16>::new("SMTP port:")
                .with_default(global.smtp_port)
                .prompt()?;
        }

        handles::update_global(pool, global.clone()).await?;

        let mut channel = handles::select_channel(pool, &1).await?;
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

        handles::update_channel(pool, 1, channel).await?;

        #[cfg(target_family = "unix")]
        {
            update_permissions().await;
        }

        println!("\nSet global settings done...");
    } else if args.user_set {
        global_user(&mut args);
    }

    if let Some(username) = args.username {
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

        handles::insert_or_update_user(pool, ff_user).await?;

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
    }

    if ARGS.dump_advanced {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                if let Err(e) = AdvancedConfig::dump(pool, *id).await {
                    return Err(ProcessError::Custom(format!("Dump config: {e}")));
                };
            }
        } else {
            return Err(ProcessError::Custom(
                "Channel ID(s) needed! Use `--channel 1 ...`".to_string(),
            ));
        }
    }

    if ARGS.dump_config {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                match PlayoutConfig::dump(pool, *id).await {
                    Ok(_) => println!("Dump config to: ffplayout_{id}.toml"),
                    Err(e) => return Err(ProcessError::Custom(format!("Dump config: {e}"))),
                };
            }
        } else {
            return Err(ProcessError::Custom(
                "Channel ID(s) needed! Use `--channel 1 ...`".to_string(),
            ));
        }
    }

    if let Some(path) = &ARGS.import_advanced {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                match AdvancedConfig::import(pool, *id, path).await {
                    Ok(_) => println!("Import config done..."),
                    Err(e) => return Err(ProcessError::Custom(format!("{e}"))),
                };
            }
        } else {
            return Err(ProcessError::Custom(
                "Channel ID(s) needed! Use `--channel 1 ...`".to_string(),
            ));
        }
    }

    if let Some(path) = &ARGS.import_config {
        if let Some(channel) = &ARGS.channel {
            for id in channel {
                match PlayoutConfig::import(pool, *id, path).await {
                    Ok(_) => println!("Import config done..."),
                    Err(e) => return Err(ProcessError::Custom(format!("{e}"))),
                };
            }
        } else {
            return Err(ProcessError::Custom(
                "Channel ID(s) needed! Use `--channel 1 ...`".to_string(),
            ));
        }
    }

    Ok(init)
}

#[cfg(target_family = "unix")]
async fn update_permissions() {
    use crate::db::DB_PATH;

    let db_path = DB_PATH.as_ref().unwrap();
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
