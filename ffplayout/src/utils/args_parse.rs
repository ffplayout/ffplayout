use std::path::PathBuf;

use clap::Parser;

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
