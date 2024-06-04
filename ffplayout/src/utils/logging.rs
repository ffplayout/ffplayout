use std::{
    collections::HashMap,
    io::{self, ErrorKind, Write},
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
    thread::{self, sleep},
    time::Duration,
};

use actix_web::{rt::time::interval, web};
use chrono::prelude::*;
use flexi_logger::writers::{FileLogWriter, LogWriter};
use flexi_logger::{Age, Cleanup, Criterion, DeferredNow, FileSpec, Logger, Naming};
use lettre::{
    message::header, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use log::{kv::Value, *};
use paris::formatter::colorize_string;

use crate::utils::{
    config::{Logging, PlayoutConfig},
    control::ProcessControl,
};

pub struct LogConsole;

impl LogWriter for LogConsole {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        console_formatter(&mut std::io::stderr(), now, record)?;

        println!();
        Ok(())
    }
    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}

struct MultiFileLogger {
    config: Logging,
    writers: Arc<Mutex<HashMap<String, Arc<Mutex<FileLogWriter>>>>>,
}

impl MultiFileLogger {
    pub fn new(config: &Logging) -> Self {
        MultiFileLogger {
            config: config.clone(),
            writers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_writer(&self, channel: &str) -> io::Result<Arc<Mutex<FileLogWriter>>> {
        let mut writers = self.writers.lock().unwrap();
        if !writers.contains_key(channel) {
            let writer = FileLogWriter::builder(
                FileSpec::default()
                    .suppress_timestamp()
                    .directory(self.config.path.clone())
                    .basename("ffplayout")
                    .discriminant(channel),
            )
            .format(file_formatter)
            .append()
            .rotate(
                Criterion::Age(Age::Day),
                Naming::TimestampsDirect,
                Cleanup::KeepLogFiles(self.config.backup_count),
            )
            .try_build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            writers.insert(channel.to_string(), Arc::new(Mutex::new(writer)));
        }
        Ok(writers.get(channel).unwrap().clone())
    }
}

impl LogWriter for MultiFileLogger {
    fn write(&self, now: &mut DeferredNow, record: &Record) -> io::Result<()> {
        let channel = record
            .key_values()
            .get("channel".into())
            .unwrap_or(Value::null())
            .to_string();
        let writer = self.get_writer(&channel);
        let w = writer?.lock().unwrap().write(now, record);

        w
    }

    fn flush(&self) -> io::Result<()> {
        let writers = self.writers.lock().unwrap();
        for writer in writers.values() {
            writer.lock().unwrap().flush()?;
        }
        Ok(())
    }
}

fn console_formatter(w: &mut dyn Write, _now: &mut DeferredNow, record: &Record) -> io::Result<()> {
    let level = match record.level() {
        Level::Debug => colorize_string("<bright magenta>[DEBUG]</>"),
        Level::Error => colorize_string("<bright red>[ERROR]</>"),
        Level::Info => colorize_string("<bright green>[ INFO]</>"),
        Level::Trace => colorize_string("<bright yellow>[TRACE]</>"),
        Level::Warn => colorize_string("<yellow>[ WARN]</>"),
    };

    write!(
        w,
        "{} {}",
        level,
        colorize_string(record.args().to_string()),
    )
}

fn file_formatter(
    w: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record,
) -> std::io::Result<()> {
    write!(
        w,
        "[{}] {} {}",
        now.now().format("%Y-%m-%d %H:%M:%S%.6f"),
        record.level(),
        record.args()
    )
}

fn file_logger(config: &Logging) -> Box<dyn LogWriter> {
    if config.log_to_file {
        let logger = MultiFileLogger::new(config);

        Box::new(logger)
    } else {
        Box::new(LogConsole)
    }
}

/// send log messages to mail recipient
pub async fn send_mail(config: &PlayoutConfig, msg: String) {
    let recipient = config
        .mail
        .recipient
        .split_terminator([',', ';', ' '])
        .filter(|s| s.contains('@'))
        .map(|s| s.trim())
        .collect::<Vec<&str>>();

    let mut message = Message::builder()
        .from(config.mail.sender_addr.parse().unwrap())
        .subject(&config.mail.subject)
        .header(header::ContentType::TEXT_PLAIN);

    for r in recipient {
        message = message.to(r.parse().unwrap());
    }

    if let Ok(mail) = message.body(msg) {
        let credentials = Credentials::new(
            config.mail.sender_addr.clone(),
            config.mail.sender_pass.clone(),
        );

        let mut transporter =
            AsyncSmtpTransport::<Tokio1Executor>::relay(config.mail.smtp_server.clone().as_str());

        if config.mail.starttls {
            transporter = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(
                config.mail.smtp_server.clone().as_str(),
            );
        }

        let mailer = transporter.unwrap().credentials(credentials).build();

        // Send the mail
        if let Err(e) = mailer.send(&mail).await? {
            error!(target: "{file}", channel = 1; "Could not send mail: {e}");
        }
    } else {
        error!(target: "{file}", channel = 1; "Mail Message failed!");
    }
}

/// Basic Mail Queue
///
/// Check every give seconds for messages and send them.
fn mail_queue(config: PlayoutConfig, messages: Arc<Mutex<Vec<String>>>) {
    let sec = config.mail.interval;

    actix_web::rt::spawn(async move {
        let mut interval = interval(Duration::from_secs(sec));

        loop {
            let mut msg = messages.lock().unwrap();

            if msg.len() > 0 {
                send_mail(&config, msg.join("\n")).await;

                msg.clear();
            }

            drop(msg);

            interval.tick().await;
        }
    });
}

/// Initialize our logging, to have:
///
/// - console logger
/// - file logger
/// - mail logger
pub fn init_logging(
    config: &PlayoutConfig,
    proc_ctl: Option<ProcessControl>,
    messages: Option<Arc<Mutex<Vec<String>>>>,
) -> io::Result<()> {
    Logger::try_with_str(config.logging.level.as_str())
        .map_err(|e| io::Error::new(ErrorKind::Other, e.to_string()))?
        .format(console_formatter)
        .log_to_stderr()
        .add_writer("file", file_logger(&config.logging))
        // .add_writer("Mail", Box::new(LogMailer))
        .start()
        .map_err(|e| io::Error::new(ErrorKind::Other, e.to_string()))?;

    Ok(())
}
