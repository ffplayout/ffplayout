extern crate log;
extern crate simplelog;

use std::{
    path::Path,
    sync::{atomic::Ordering, Arc, Mutex},
    thread::{self, sleep},
    time::Duration,
};

use chrono::prelude::*;
use file_rotate::{
    compression::Compression,
    suffix::{AppendTimestamp, DateFrom, FileLimit},
    ContentLimit, FileRotate, TimeFrequency,
};
use lettre::{
    message::header, transport::smtp::authentication::Credentials, Message, SmtpTransport,
    Transport,
};
use log::{Level, LevelFilter, Log, Metadata, Record};
use regex::Regex;
use simplelog::*;

use crate::utils::{GlobalConfig, ProcessControl};

/// send log messages to mail recipient
pub fn send_mail(cfg: &GlobalConfig, msg: String) {
    if let Ok(email) = Message::builder()
        .from(cfg.mail.sender_addr.parse().unwrap())
        .to(cfg.mail.recipient.parse().unwrap())
        .subject(cfg.mail.subject.clone())
        .header(header::ContentType::TEXT_PLAIN)
        .body(clean_string(&msg))
    {
        let credentials =
            Credentials::new(cfg.mail.sender_addr.clone(), cfg.mail.sender_pass.clone());

        let mut transporter = SmtpTransport::relay(cfg.mail.smtp_server.clone().as_str());

        if cfg.mail.starttls {
            transporter = SmtpTransport::starttls_relay(cfg.mail.smtp_server.clone().as_str())
        }

        let mailer = transporter.unwrap().credentials(credentials).build();

        // Send the email
        if let Err(e) = mailer.send(&email) {
            error!("Could not send email: {:?}", e)
        }
    } else {
        error!("Mail Message failed!")
    }
}

/// Basic Mail Queue
///
/// Check every give seconds for messages and send them.
fn mail_queue(
    cfg: GlobalConfig,
    proc_ctl: ProcessControl,
    messages: Arc<Mutex<Vec<String>>>,
    interval: u64,
) {
    while !proc_ctl.is_terminated.load(Ordering::SeqCst) {
        if messages.lock().unwrap().len() > 0 {
            let msg = messages.lock().unwrap().join("\n");
            send_mail(&cfg, msg);

            messages.lock().unwrap().clear();
        }

        sleep(Duration::from_secs(interval));
    }
}

/// Self made Mail Log struct, to extend simplelog.
pub struct LogMailer {
    level: LevelFilter,
    pub config: Config,
    messages: Arc<Mutex<Vec<String>>>,
    last_message: Arc<Mutex<String>>,
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
            last_message: Arc::new(Mutex::new(String::new())),
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
            let mut last_msg = self.last_message.lock().unwrap();

            // put message only to mail queue when it differs from last message
            // this we do to prevent spamming the mail box
            if *last_msg != rec {
                *last_msg = rec.clone();
                let full_line: String = format!("{time_stamp} [{level: >5}] {rec}");

                self.messages.lock().unwrap().push(full_line);
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

/// Workaround to remove color information from log
///
/// ToDo: maybe in next version from simplelog this is not necessary anymore.
fn clean_string(text: &str) -> String {
    let regex = Regex::new(r"\x1b\[[0-9;]*[mGKF]").unwrap();

    regex.replace_all(text, "").to_string()
}

/// Initialize our logging, to have:
///
/// - console logger
/// - file logger
/// - mail logger
pub fn init_logging(
    config: &GlobalConfig,
    proc_ctl: ProcessControl,
    messages: Arc<Mutex<Vec<String>>>,
) -> Vec<Box<dyn SharedLogger>> {
    let config_clone = config.clone();
    let app_config = config.logging.clone();
    let mut time_level = LevelFilter::Off;
    let mut app_logger: Vec<Box<dyn SharedLogger>> = vec![];

    if app_config.timestamp {
        time_level = LevelFilter::Error;
    }

    let mut log_config = ConfigBuilder::new()
        .set_thread_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .set_level_padding(LevelPadding::Left)
        .set_time_level(time_level)
        .clone();

    if app_config.local_time {
        log_config = match log_config.set_time_offset_to_local() {
            Ok(local) => local.clone(),
            Err(_) => log_config,
        };
    };

    if app_config.log_to_file {
        let file_config = log_config
            .clone()
            .set_time_format_custom(format_description!(
                "[[[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:5]]"
            ))
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

        let log_file = FileRotate::new(
            log_path,
            AppendTimestamp::with_format(
                "%Y-%m-%d",
                FileLimit::MaxFiles(app_config.backup_count),
                DateFrom::DateYesterday,
            ),
            ContentLimit::Time(TimeFrequency::Daily),
            Compression::None,
        );

        app_logger.push(WriteLogger::new(LevelFilter::Debug, file_config, log_file));
    } else {
        let term_config = log_config
            .clone()
            .set_level_color(Level::Debug, Some(Color::Ansi256(12)))
            .set_level_color(Level::Info, Some(Color::Ansi256(10)))
            .set_level_color(Level::Warn, Some(Color::Ansi256(208)))
            .set_level_color(Level::Error, Some(Color::Ansi256(9)))
            .set_time_format_custom(format_description!(
                "\x1b[[30;1m[[[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:5]]\x1b[[0m"
            ))
            .build();

        app_logger.push(TermLogger::new(
            LevelFilter::Debug,
            term_config,
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ));
    }

    // set mail logger only the recipient is set in config
    if config.mail.recipient.contains('@') && config.mail.recipient.contains('.') {
        let messages_clone = messages.clone();
        let interval = config.mail.interval;

        thread::spawn(move || mail_queue(config_clone, proc_ctl, messages_clone, interval));

        let mail_config = log_config.build();

        let filter = match config.mail.mail_level.to_lowercase().as_str() {
            "info" => LevelFilter::Info,
            "warning" => LevelFilter::Warn,
            _ => LevelFilter::Error,
        };

        app_logger.push(LogMailer::new(filter, mail_config, messages));
    }

    app_logger
}
