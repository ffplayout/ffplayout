use std::{
    collections::{hash_map, HashMap},
    env,
    io::{self, ErrorKind, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use actix_web::rt::time::interval;
use flexi_logger::{
    writers::{FileLogWriter, LogWriter},
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

#[derive(Debug)]
pub struct Target;

impl Target {
    pub fn console() -> &'static str {
        "{console}"
    }

    pub fn file() -> &'static str {
        "{file}"
    }

    pub fn mail() -> &'static str {
        "{mail}"
    }

    pub fn file_mail() -> &'static str {
        "{file,mail}"
    }
}

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
        if let hash_map::Entry::Vacant(e) = writers.entry(channel) {
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
                Naming::TimestampsCustomFormat {
                    current_infix: Some(""),
                    format: "%Y-%m-%d",
                },
                Cleanup::KeepLogFiles(ARGS.log_backup_count.unwrap_or(14)),
            )
            .try_build()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            e.insert(Arc::new(Mutex::new(writer)));
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
    pub mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
}

impl LogMailer {
    pub fn new(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) -> Self {
        Self { mail_queues }
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

        let mut queues = self.mail_queues.lock().unwrap_or_else(|poisoned| {
            error!("Queues mutex was poisoned");
            poisoned.into_inner()
        });

        for queue in queues.iter_mut() {
            let mut q_lock = queue.lock().unwrap_or_else(|poisoned| {
                error!("Queue mutex was poisoned");
                poisoned.into_inner()
            });

            if q_lock.id == id && q_lock.level_eq(record.level()) {
                q_lock.push(format!(
                    "[{}] [{:>5}] {}",
                    now.now().format("%Y-%m-%d %H:%M:%S"),
                    record.level(),
                    record.args()
                ));

                break;
            }
        }

        Ok(())
    }
    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct MailQueue {
    pub id: i32,
    pub config: Mail,
    pub lines: Vec<String>,
}

impl MailQueue {
    pub fn new(id: i32, config: Mail) -> Self {
        Self {
            id,
            config,
            lines: vec![],
        }
    }

    pub fn level_eq(&self, level: Level) -> bool {
        match level {
            Level::Error => self.config.mail_level == Level::Error,
            Level::Warn => matches!(self.config.mail_level, Level::Warn | Level::Error),
            Level::Info => matches!(
                self.config.mail_level,
                Level::Info | Level::Warn | Level::Error
            ),
            _ => false,
        }
    }

    pub fn update(&mut self, config: Mail) {
        self.config = config;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn push(&mut self, line: String) {
        self.lines.push(line);
    }

    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
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

pub fn log_file_path() -> PathBuf {
    let mut log_path = ARGS
        .log_path
        .clone()
        .unwrap_or(PathBuf::from("/var/log/ffplayout"));

    if !log_path.is_dir() {
        log_path = env::current_dir().unwrap();
    }

    log_path
}

fn file_logger() -> Box<dyn LogWriter> {
    if ARGS.log_to_console {
        Box::new(LogConsole)
    } else {
        Box::new(MultiFileLogger::new(log_file_path()))
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
pub fn mail_queue(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) {
    actix_web::rt::spawn(async move {
        let sec = 10;
        let mut interval = interval(Duration::from_secs(sec));
        let mut counter = 0;

        loop {
            interval.tick().await;
            let mut tasks = vec![];

            // Reset the counter after one day
            if counter >= 86400 {
                counter = 0;
            } else {
                counter += sec;
            }

            {
                let mut queues = match mail_queues.lock() {
                    Ok(l) => l,
                    Err(e) => {
                        error!("Failed to lock mail_queues {e}");
                        continue;
                    }
                };

                // Process mail queues and send emails
                for queue in queues.iter_mut() {
                    let interval = round_to_nearest_ten(counter);
                    let mut q_lock = queue.lock().unwrap_or_else(|poisoned| {
                        error!("Queue mutex was poisoned");

                        poisoned.into_inner()
                    });

                    let expire = round_to_nearest_ten(q_lock.config.interval);

                    if interval % expire == 0 && !q_lock.is_empty() {
                        if q_lock.config.recipient.contains('@') {
                            tasks.push((q_lock.config.clone(), q_lock.text().clone(), q_lock.id));
                        }

                        // Clear the messages after sending the email
                        q_lock.clear();
                    }
                }
            }

            for (config, text, id) in tasks {
                if let Err(e) = send_mail(&config, text).await {
                    error!(target: "{file}", channel = id; "Failed to send mail: {e}");
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
pub fn init_logging(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) -> io::Result<()> {
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

    mail_queue(mail_queues.clone());

    // Build the initial log specification
    let mut builder = LogSpecification::builder();
    builder
        .default(log_level)
        .module("actix_files", LevelFilter::Error)
        .module("actix_web", LevelFilter::Error)
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
        .add_writer("mail", Box::new(LogMailer::new(mail_queues)))
        .start()
        .map_err(|e| io::Error::new(ErrorKind::Other, e.to_string()))?;

    Ok(())
}

/// Format ingest and HLS logging output
pub fn log_line(line: &str, level: &str) {
    if line.contains("[info]") && level.to_lowercase() == "info" {
        info!("<bright black>[Server]</> {}", line.replace("[info] ", ""))
    } else if line.contains("[warning]")
        && (level.to_lowercase() == "warning" || level.to_lowercase() == "info")
    {
        warn!(
            "<bright black>[Server]</> {}",
            line.replace("[warning] ", "")
        )
    } else if line.contains("[error]")
        && !line.contains("Input/output error")
        && !line.contains("Broken pipe")
    {
        error!("<bright black>[Server]</> {}", line.replace("[error] ", ""));
    } else if line.contains("[fatal]") {
        error!("<bright black>[Server]</> {}", line.replace("[fatal] ", ""))
    }
}
