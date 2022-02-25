extern crate log;
extern crate simplelog;

use std::path::Path;

use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};
use log::{Level, LevelFilter, Log, Metadata, Record};
use simplelog::*;

use crate::utils;

pub struct LogMailer {
    level: LevelFilter,
    config: Config,
}

impl LogMailer {
    pub fn new(log_level: LevelFilter, config: Config) -> Box<LogMailer> {
        Box::new(LogMailer {
            level: log_level,
            config,
        })
    }
}

impl Log for LogMailer {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            match record.level() {
                Level::Error => {
                    println!("Send Error Mail: {:?}\n{:?}", record, self.config)
                }
                Level::Warn => {
                    println!("Send Warn Mail: {:?}", record.args())
                }
                _ => (),
            }
        }
    }

    fn flush(&self) {}
}

impl SharedLogger for LogMailer {
    fn level(&self) -> LevelFilter {
        self.level
    }

    fn config(&self) -> Option<&Config> {
        Some(&self.config)
    }

    fn as_log(self: Box<Self>) -> Box<dyn Log> {
        Box::new(*self)
    }
}

pub fn init_logging(config: &utils::Config) -> Vec<Box<dyn SharedLogger>> {
    let app_config = config.logging.clone();
    let mut app_logger: Vec<Box<dyn SharedLogger>> = vec![];

    let log_config = simplelog::ConfigBuilder::new()
        .set_thread_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .set_level_padding(LevelPadding::Left)
        .set_time_to_local(app_config.local_time)
        .clone();

    if app_config.log_to_file {
        let file_config = log_config
            .clone()
            .set_time_format("[%Y-%m-%d %H:%M:%S%.3f]".into())
            .build();
        let mut log_path = "logs/ffplayout.log".to_string();

        if Path::new(&app_config.log_path).is_dir() {
            log_path = Path::new(&app_config.log_path)
                .join("ffplayout.log")
                .display()
                .to_string();
        } else if Path::new(&app_config.log_path).is_file() {
            log_path = app_config.log_path
        } else {
            println!("Logging path not exists!")
        }

        let log = || {
            FileRotate::new(
                log_path,
                AppendCount::new(app_config.backup_count),
                ContentLimit::Lines(1000),
                Compression::None,
            )
        };

        app_logger.push(WriteLogger::new(LevelFilter::Debug, file_config, log()));
    } else {
        let term_config = log_config
            .clone()
            .set_level_color(Level::Debug, Some(Color::Ansi256(12)))
            .set_level_color(Level::Info, Some(Color::Ansi256(10)))
            .set_level_color(Level::Warn, Some(Color::Ansi256(208)))
            .set_level_color(Level::Error, Some(Color::Ansi256(9)))
            .set_time_format_str("\x1b[30;1m[%Y-%m-%d %H:%M:%S%.3f]\x1b[0m")
            .build();

        app_logger.push(TermLogger::new(
            LevelFilter::Debug,
            term_config,
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ));
    }

    if config.mail.recipient.len() > 3 {
        let mail_config = log_config
            .clone()
            .set_time_format_str("[%Y-%m-%d %H:%M:%S%.3f]")
            .build();

        app_logger.push(LogMailer::new(LevelFilter::Warn, mail_config));
    }

    app_logger
}
