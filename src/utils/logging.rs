extern crate log;
extern crate simplelog;

use chrono::prelude::*;
use regex::Regex;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    thread::sleep,
    time::Duration,
};

use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};
use lettre::{
    message::header, transport::smtp::authentication::Credentials, Message, SmtpTransport,
    Transport,
};
use log::{Level, LevelFilter, Log, Metadata, Record};
use simplelog::*;
use tokio::runtime::Handle;

use crate::utils::GlobalConfig;

fn send_mail(msg: String) {
    let config = GlobalConfig::global();

    let email = Message::builder()
        .from(config.mail.sender_addr.parse().unwrap())
        .to(config.mail.recipient.parse().unwrap())
        .subject(config.mail.subject.clone())
        .header(header::ContentType::TEXT_PLAIN)
        .body(clean_string(&msg))
        .unwrap();

    let credentials = Credentials::new(
        config.mail.sender_addr.clone(),
        config.mail.sender_pass.clone(),
    );

    let mut transporter = SmtpTransport::relay(config.mail.smtp_server.clone().as_str());

    if config.mail.starttls {
        transporter = SmtpTransport::starttls_relay(config.mail.smtp_server.clone().as_str())
    }

    let mailer = transporter.unwrap().credentials(credentials).build();

    // Send the email
    match mailer.send(&email) {
        Ok(_) => (),
        Err(e) => info!("Could not send email: {:?}", e),
    }
}

async fn mail_queue(
    messages: Arc<Mutex<Vec<String>>>,
    is_terminated: Arc<Mutex<bool>>,
    interval: i32,
) {
    let mut count: i32 = 0;

    loop {
        if *is_terminated.lock().unwrap() || count == interval {
            // check every 30 seconds for messages and send them
            if messages.lock().unwrap().len() > 0 {
                let msg = messages.lock().unwrap().join("\n");
                send_mail(msg);

                messages.lock().unwrap().clear();
            }

            count = 0;
        }

        if *is_terminated.lock().unwrap() {
            break;
        }

        sleep(Duration::from_secs(1));
        count += 1;
    }
}

pub struct LogMailer {
    level: LevelFilter,
    pub config: Config,
    messages: Arc<Mutex<Vec<String>>>,
}

impl LogMailer {
    pub fn new(
        log_level: LevelFilter,
        config: Config,
        messages: Arc<Mutex<Vec<String>>>,
    ) -> Box<LogMailer> {
        Box::new(LogMailer {
            level: log_level,
            config,
            messages,
        })
    }
}

impl Log for LogMailer {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            let local: DateTime<Local> = Local::now();
            let time_stamp = local.format("[%Y-%m-%d %H:%M:%S%.3f]");
            let level = record.level().to_string().to_uppercase();
            let rec = record.args().to_string();
            let full_line: String = format!("{time_stamp} [{level: >5}] {rec}");

            self.messages.lock().unwrap().push(full_line);
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

fn clean_string(text: &str) -> String {
    let regex: Regex = Regex::new(r"\x1b\[[0-9;]*[mGKF]").unwrap();

    regex.replace_all(text, "").to_string()
}

pub fn init_logging(
    rt_handle: Handle,
    is_terminated: Arc<Mutex<bool>>,
) -> Vec<Box<dyn SharedLogger>> {
    let config = GlobalConfig::global();
    let app_config = config.logging.clone();
    let mut time_level = LevelFilter::Off;
    let mut app_logger: Vec<Box<dyn SharedLogger>> = vec![];

    if app_config.timestamp {
        time_level = LevelFilter::Error;
    }

    let log_config = simplelog::ConfigBuilder::new()
        .set_thread_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .set_level_padding(LevelPadding::Left)
        .set_time_to_local(app_config.local_time)
        .set_time_level(time_level)
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
        let messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let interval = config.mail.interval.clone();

        rt_handle.spawn(mail_queue(
            messages.clone(),
            is_terminated.clone(),
            interval,
        ));

        let mail_config = log_config
            .clone()
            .build();

        let filter = match config.mail.mail_level.to_lowercase().as_str() {
            "info" => LevelFilter::Info,
            "warning" => LevelFilter::Warn,
            _ => LevelFilter::Error,
        };

        app_logger.push(LogMailer::new(filter, mail_config, messages));
    }

    app_logger
}
