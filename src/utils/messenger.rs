extern crate log;
extern crate simplelog;

use simplelog::*;
use std::fs::File;

use crate::utils::Config;

pub struct Messenger {
    message: String,
    log_to_file: bool,
    backup_count: u32,
    path: String,
    level: String,
    ffmpeg_level: String,
}

impl Messenger {
    pub fn new(config: &Config) -> Self {
        let conf = config.logging.clone();

        let logger_config = simplelog::ConfigBuilder::new()
            .set_level_color(Level::Info, Some(Color::Green))
            .build();

        if conf.log_to_file {
            WriteLogger::init(
                LevelFilter::Debug,
                simplelog::Config::default(),
                File::create("ffplayout.log").unwrap(),
            )
            .unwrap();
        } else {
            TermLogger::init(
                LevelFilter::Debug,
                logger_config,
                TerminalMode::Mixed,
                ColorChoice::Auto,
            )
            .unwrap();
        }

        Messenger {
            message: "".to_string(),
            log_to_file: conf.log_to_file,
            backup_count: conf.backup_count,
            path: conf.log_path,
            level: conf.log_level,
            ffmpeg_level: conf.ffmpeg_level,
        }
    }

    pub fn debug(&self, msg: &str) {
        if self.level.to_lowercase() == "debug".to_string() {
            debug!("{}", msg)
        }
    }

    pub fn info(&self, msg: &str) {
        if self.level.to_lowercase() == "debug".to_string()
            || self.level.to_lowercase() == "info".to_string()
        {
            info!("{}", msg)
        }
    }

    pub fn warning(&self, msg: &str) {
        if self.level.to_lowercase() == "debug".to_string()
            || self.level.to_lowercase() == "info".to_string()
            || self.level.to_lowercase() == "warning".to_string()
        {
            warn!("{}", msg)
        }
    }

    pub fn error(&self, msg: &str) {
        if self.level.to_lowercase() == "debug".to_string()
            || self.level.to_lowercase() == "info".to_string()
            || self.level.to_lowercase() == "warning".to_string()
            || self.level.to_lowercase() == "error".to_string()
        {
            error!("{}", msg)
        }
    }
}
