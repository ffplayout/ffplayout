extern crate log;
extern crate simplelog;

use std::path::Path;

use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};
use simplelog::*;

use crate::utils::Config;

#[derive(Debug, Clone)]
pub struct Messenger {
    level: String,
    // ffmpeg_level: String,
}

impl Messenger {
    pub fn new(config: &Config) -> Self {
        let conf = config.logging.clone();
        let log_config = simplelog::ConfigBuilder::new()
            .set_thread_level(LevelFilter::Off)
            .set_target_level(LevelFilter::Off)
            .set_level_padding(LevelPadding::Left)
            .set_time_to_local(conf.local_time)
            .clone();

        if conf.log_to_file {
            let file_config = log_config
                .clone()
                .set_time_format("[%Y-%m-%d %H:%M:%S%.3f]".into())
                .build();
            let mut log_path = "logs/ffplayout.log".to_string();

            if Path::new(&conf.log_path).is_dir() {
                log_path = Path::new(&conf.log_path)
                    .join("ffplayout.log")
                    .display()
                    .to_string();
            } else if Path::new(&conf.log_path).is_file() {
                log_path = conf.log_path
            } else {
                println!("Logging path not exists!")
            }

            let log = || {
                FileRotate::new(
                    log_path,
                    AppendCount::new(conf.backup_count),
                    ContentLimit::Lines(1000),
                    Compression::None,
                )
            };

            WriteLogger::init(LevelFilter::Debug, file_config, log()).unwrap();
        } else {
            let term_config = log_config
                .clone()
                .set_level_color(Level::Debug, Some(Color::Ansi256(12)))
                .set_level_color(Level::Info, Some(Color::Ansi256(10)))
                .set_level_color(Level::Warn, Some(Color::Ansi256(208)))
                .set_level_color(Level::Error, Some(Color::Ansi256(9)))
                .set_time_format_str("\x1b[30;1m[%Y-%m-%d %H:%M:%S%.3f]\x1b[0m")
                .build();

            TermLogger::init(
                LevelFilter::Debug,
                term_config,
                TerminalMode::Mixed,
                ColorChoice::Auto,
            )
            .unwrap();
        }

        Messenger {
            level: conf.log_level,
            // ffmpeg_level: conf.ffmpeg_level,
        }
    }

    pub fn debug(&self, msg: String) {
        if self.level.to_lowercase() == "debug".to_string() {
            debug!("{}", msg)
        }
    }

    pub fn info(&self, msg: String) {
        if self.level.to_lowercase() == "debug".to_string()
            || self.level.to_lowercase() == "info".to_string()
        {
            info!("{}", msg)
        }
    }

    pub fn warning(&self, msg: String) {
        if self.level.to_lowercase() == "debug".to_string()
            || self.level.to_lowercase() == "info".to_string()
            || self.level.to_lowercase() == "warning".to_string()
        {
            warn!("{}", msg)
        }
    }

    pub fn error(&self, msg: String) {
        if self.level.to_lowercase() == "debug".to_string()
            || self.level.to_lowercase() == "info".to_string()
            || self.level.to_lowercase() == "warning".to_string()
            || self.level.to_lowercase() == "error".to_string()
        {
            error!("{}", msg)
        }
    }
}
