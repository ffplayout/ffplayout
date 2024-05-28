use log::{LevelFilter, Log, Metadata, Record};
use simplelog::*;
use std::fs::File;

pub struct LogMailer {
    level: LevelFilter,
    pub config: Config,
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
            let _rec = record.args().to_string();

            println!("{record:?}");
            println!("target: {:?}", record.target());
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

struct Log2 {
    logger: Box<WriteLogger<File>>,
}

impl Log2 {
    fn new() -> Self {
        let log_file = File::create("log_file.log").expect("Failed to create log file");

        let config = ConfigBuilder::new()
            .set_time_format_custom(format_description!(
                "[[[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:5]]"
            ))
            .build();

        let logger = WriteLogger::new(LevelFilter::Debug, config, log_file);

        Log2 { logger }
    }

    fn debug(&self, message: &str) {
        self.logger.log(
            &Record::builder()
                .args(format_args!("{}", message))
                .level(Level::Debug)
                .build(),
        );
    }
}

fn main() {
    let log2 = Log2::new();

    log2.debug("Debug-Message in Logger 2");

    // std::thread::spawn(move || {
    //     log2.debug("Error-Message in Logger 2");
    // });

    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Debug,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        LogMailer::new(LevelFilter::Info, Config::default()),
    ])
    .unwrap();

    info!("Info in Logger 1");
    warn!("Warning in Logger 1");
}
