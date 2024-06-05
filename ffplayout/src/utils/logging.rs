use std::{
    collections::HashMap,
    env,
    io::{self, ErrorKind, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use actix_web::rt::time::interval;
use flexi_logger::writers::{FileLogWriter, LogWriter};
use flexi_logger::{
    Age, Cleanup, Criterion, DeferredNow, FileSpec, Level, LogSpecification, Logger, Naming,
};
use lettre::{
    message::header, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use log::{kv::Value, *};
use paris::formatter::colorize_string;

use super::ARGS;

use crate::utils::{config::Mail, errors::ProcessError, round_to_nearest_ten};

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
    log_path: PathBuf,
    writers: Arc<Mutex<HashMap<i32, Arc<Mutex<FileLogWriter>>>>>,
}

impl MultiFileLogger {
    pub fn new(log_path: PathBuf) -> Self {
        MultiFileLogger {
            log_path,
            writers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_writer(&self, channel: i32) -> io::Result<Arc<Mutex<FileLogWriter>>> {
        let mut writers = self.writers.lock().unwrap();
        if !writers.contains_key(&channel) {
            let writer = FileLogWriter::builder(
                FileSpec::default()
                    .suppress_timestamp()
                    .directory(&self.log_path)
                    .basename("ffplayout")
                    .discriminant(channel.to_string()),
            )
            .format(file_formatter)
            .append()
            .rotate(
                Criterion::Age(Age::Day),
                Naming::Timestamps,
                Cleanup::KeepLogFiles(ARGS.log_backup_count.unwrap_or(14)),
            )
            .try_build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            writers.insert(channel, Arc::new(Mutex::new(writer)));
        }
        Ok(writers.get(&channel).unwrap().clone())
    }
}

impl LogWriter for MultiFileLogger {
    fn write(&self, now: &mut DeferredNow, record: &Record) -> io::Result<()> {
        let channel = i32::try_from(
            record
                .key_values()
                .get("channel".into())
                .unwrap_or(Value::null())
                .to_i64()
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let writer = self.get_writer(channel);
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

pub struct LogMailer {
    pub messages: Arc<Mutex<Vec<MailMessage>>>,
}

impl LogMailer {
    pub fn new(messages: Arc<Mutex<Vec<MailMessage>>>) -> Self {
        Self { messages }
    }

    fn push(&self, msg: MailMessage) {
        if let Ok(mut list) = self.messages.lock() {
            list.push(msg)
        }
    }
}

impl LogWriter for LogMailer {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        let id = i32::try_from(
            record
                .key_values()
                .get("channel".into())
                .unwrap_or(Value::null())
                .to_i64()
                .unwrap_or(0),
        )
        .unwrap_or(0);

        let msg = MailMessage::new(
            id,
            record.level(),
            format!(
                "[{}] [{:>5}] {}",
                now.now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            ),
        );

        self.push(msg.clone());

        Ok(())
    }
    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct MailQueue {
    pub id: i32,
    pub expire: u64,
    pub config: Mail,
    pub lines: Vec<String>,
}

impl MailQueue {
    pub fn new(id: i32, expire: u64, config: Mail) -> Self {
        Self {
            id,
            expire,
            config,
            lines: vec![],
        }
    }

    pub fn update(&mut self, expire: u64, config: Mail) {
        self.expire = expire;
        self.config = config;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct MailMessage {
    pub id: i32,
    pub level: Level,
    pub line: String,
}

impl MailMessage {
    pub fn new(id: i32, level: Level, line: String) -> Self {
        Self { id, level, line }
    }

    fn eq(&self, level: Level) -> bool {
        match level {
            Level::Error => self.level == Level::Error,
            Level::Warn => matches!(self.level, Level::Warn | Level::Error),
            Level::Info => matches!(self.level, Level::Info | Level::Warn | Level::Error),
            _ => false,
        }
    }
}

fn console_formatter(w: &mut dyn Write, _now: &mut DeferredNow, record: &Record) -> io::Result<()> {
    let level = match record.level() {
        Level::Debug => "<bright-blue>[DEBUG]</>",
        Level::Error => "<bright-red>[ERROR]</>",
        Level::Info => "<bright-green>[ INFO]</>",
        Level::Trace => "<bright-yellow>[TRACE]</>",
        Level::Warn => "<yellow>[ WARN]</>",
    };

    write!(
        w,
        "{}",
        colorize_string(format!("{level} {}", record.args()))
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

fn file_logger() -> Box<dyn LogWriter> {
    let mut log_path = ARGS
        .log_path
        .clone()
        .unwrap_or(PathBuf::from("/var/log/ffplayout"));

    if !log_path.is_dir() {
        log_path = env::current_dir().unwrap();
    }

    if ARGS.log_to_console {
        Box::new(LogConsole)
    } else {
        let logger = MultiFileLogger::new(log_path);

        Box::new(logger)
    }
}

/// send log messages to mail recipient
pub async fn send_mail(config: &Mail, msg: String) -> Result<(), ProcessError> {
    let recipient = config
        .recipient
        .split_terminator([',', ';', ' '])
        .filter(|s| s.contains('@'))
        .map(|s| s.trim())
        .collect::<Vec<&str>>();

    let mut message = Message::builder()
        .from(config.sender_addr.parse()?)
        .subject(&config.subject)
        .header(header::ContentType::TEXT_PLAIN);

    for r in recipient {
        message = message.to(r.parse()?);
    }

    let mail = message.body(msg)?;
    let credentials = Credentials::new(config.sender_addr.clone(), config.sender_pass.clone());

    let mut transporter =
        AsyncSmtpTransport::<Tokio1Executor>::relay(config.smtp_server.clone().as_str());

    if config.starttls {
        transporter = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(
            config.smtp_server.clone().as_str(),
        );
    }

    let mailer = transporter?.credentials(credentials).build();

    // Send the mail
    mailer.send(mail).await?;

    Ok(())
}

/// Basic Mail Queue
///
/// Check every give seconds for messages and send them.
pub fn mail_queue(mail_queues: Arc<Mutex<Vec<MailQueue>>>, messages: Arc<Mutex<Vec<MailMessage>>>) {
    actix_web::rt::spawn(async move {
        let sec = 10;
        let mut interval = interval(Duration::from_secs(sec));
        let mut counter = 0;

        loop {
            interval.tick().await;

            // Reset the counter after one day
            if counter >= 86400 {
                counter = 0;
            } else {
                counter += sec;
            }

            let mut msg_list = match mail_queues.lock() {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to lock mail_queues {e}");
                    continue;
                }
            };

            let mut msgs = match messages.lock() {
                Ok(m) => m,
                Err(e) => {
                    error!("Failed to lock messages {e}");
                    continue;
                }
            };

            while let Some(msg) = msgs.pop() {
                if let Some(queue) = msg_list.iter_mut().find(|q| q.id == msg.id) {
                    if msg.eq(queue.config.mail_level) {
                        queue.lines.push(msg.line.clone());
                    }
                }
            }

            // Process mail queues and send emails
            for queue in msg_list.iter_mut() {
                let interval = round_to_nearest_ten(counter);

                if interval % queue.expire == 0 && !queue.is_empty() {
                    if let Err(e) = send_mail(&queue.config, queue.text()).await {
                        error!(target: "{file}", channel = queue.id; "Failed to send mail: {e}");
                    }
                    // Clear the messages after sending the email
                    queue.clear();
                }
            }
        }
    });
}

/// Initialize our logging, to have:
///
/// - console logger
/// - file logger
/// - mail logger
pub fn init_logging(
    mail_queues: Arc<Mutex<Vec<MailQueue>>>,
    messages: Arc<Mutex<Vec<MailMessage>>>,
) -> io::Result<()> {
    let log_level = match ARGS
        .log_level
        .clone()
        .unwrap_or("debug".to_string())
        .to_lowercase()
        .as_str()
    {
        "debug" => LevelFilter::Debug,
        "error" => LevelFilter::Error,
        "info" => LevelFilter::Info,
        "trace" => LevelFilter::Trace,
        "warn" => LevelFilter::Warn,
        "off" => LevelFilter::Off,
        _ => LevelFilter::Debug,
    };

    mail_queue(mail_queues, messages.clone());

    // Build the initial log specification
    let mut builder = LogSpecification::builder();
    builder
        .default(log_level)
        .module("hyper", LevelFilter::Error)
        .module("libc", LevelFilter::Error)
        .module("neli", LevelFilter::Error)
        .module("reqwest", LevelFilter::Error)
        .module("rustls", LevelFilter::Error)
        .module("serial_test", LevelFilter::Error)
        .module("sqlx", LevelFilter::Error);

    Logger::with(builder.build())
        .format(console_formatter)
        .log_to_stderr()
        .add_writer("file", file_logger())
        .add_writer("mail", Box::new(LogMailer::new(messages)))
        .start()
        .map_err(|e| io::Error::new(ErrorKind::Other, e.to_string()))?;

    Ok(())
}
