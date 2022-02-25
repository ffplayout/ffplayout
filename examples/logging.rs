extern crate log;
extern crate simplelog;

use simplelog::*;

use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};

// use crate::{Config, SharedLogger};
use log::{Level, LevelFilter, Log, Metadata, Record};

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
                },
                Level::Warn => {
                    println!("Send Warn Mail: {:?}", record.args())
                }
                _ => ()
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

fn main() {
    let log = || {
        FileRotate::new(
            "logs/ffplayout.log",
            AppendCount::new(7),
            ContentLimit::Lines(1000),
            Compression::None,
        )
    };

    let def_config = simplelog::ConfigBuilder::new()
        .set_target_level(LevelFilter::Off)
        .set_thread_level(LevelFilter::Off)
        .set_level_padding(LevelPadding::Left)
        .set_time_to_local(true)
        .clone();

    let term_config = def_config
        .clone()
        .set_level_color(Level::Debug, Some(Color::Ansi256(12)))
        .set_level_color(Level::Info, Some(Color::Ansi256(10)))
        .set_level_color(Level::Warn, Some(Color::Ansi256(208)))
        .set_level_color(Level::Error, Some(Color::Ansi256(9)))
        .set_time_format_str("\x1b[30;1m[%Y-%m-%d %H:%M:%S%.3f]\x1b[0m")
        .build();

    let file_config = def_config
        .clone()
        .set_time_format_str("[%Y-%m-%d %H:%M:%S%.3f]")
        .build();

    let mail_config = def_config
        .clone()
        .set_time_format_str("[%Y-%m-%d %H:%M:%S%.3f]")
        .build();

    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Debug,
            term_config,
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ),
        WriteLogger::new(LevelFilter::Debug, file_config, log()),
        LogMailer::new(LevelFilter::Warn, mail_config),
    ])
    .unwrap();

    debug!("this is a <b>debug</> message");
    info!("this is a info message");
    warn!("this is a warning message");
    error!("this is a error message");

    for idx in 1..10 {
        info!("{idx}");
    }
}
