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
use regex::Regex;

use super::ARGS;

use crate::db::GLOBAL_SETTINGS;
use crate::utils::{
    config::Mail, errors::ProcessError, round_to_nearest_ten, time_machine::time_now,
};

#[derive(Debug)]
pub struct Target;

impl Target {
    pub fn all() -> &'static str {
        if ARGS.log_to_console {
            "{_Default}"
        } else {
            "{file,mail,_Default}"
        }
    }

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
        Self {
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
                .and_then(|v| Value::to_i64(&v))
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
    raw_lines: Arc<Mutex<Vec<String>>>,
}

impl LogMailer {
    pub fn new(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) -> Self {
        Self {
            mail_queues,
            raw_lines: Arc::new(Mutex::new(vec![])),
        }
    }
}

impl LogWriter for LogMailer {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        let id = i32::try_from(
            record
                .key_values()
                .get("channel".into())
                .and_then(|v| Value::to_i64(&v))
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

            let msg = strip_tags(&record.args().to_string());
            let mut raw_lines = self.raw_lines.lock().unwrap();

            if q_lock.id == id && q_lock.level_eq(record.level()) && !raw_lines.contains(&msg) {
                q_lock.push(format!(
                    "[{}] [{:>5}] {}",
                    now.now().format("%Y-%m-%d %H:%M:%S"),
                    record.level(),
                    msg
                ));
                raw_lines.push(msg);

                break;
            }

            if raw_lines.len() > 1000 {
                let last = raw_lines.pop().unwrap();
                raw_lines.clear();
                raw_lines.push(last);
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
        level <= self.config.mail_level
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

fn strip_tags(input: &str) -> String {
    let re = Regex::new(r"<[^>]*>").unwrap();
    re.replace_all(input, "").to_string()
}

fn console_formatter(w: &mut dyn Write, now: &mut DeferredNow, record: &Record) -> io::Result<()> {
    let log_line = match record.level() {
        Level::Debug => colorize_string(format!("<bright-blue>[DEBUG]</> {}", record.args())),
        Level::Error => colorize_string(format!("<bright-red>[ERROR]</> {}", record.args())),
        Level::Info => colorize_string(format!("<bright-green>[ INFO]</> {}", record.args())),
        Level::Trace => colorize_string(format!(
            "<bright-yellow>[TRACE]</> {}:{} {}",
            record.file().unwrap_or_default(),
            record.line().unwrap_or_default(),
            record.args()
        )),
        Level::Warn => colorize_string(format!("<yellow>[ WARN]</> {}", record.args())),
    };

    if ARGS.log_timestamp {
        let time = if ARGS.fake_time.is_some() {
            time_now()
        } else {
            *now.now()
        };

        write!(
            w,
            "{} {}",
            colorize_string(format!(
                "<bright black>[{}]</>",
                time.format("%Y-%m-%d %H:%M:%S%.6f")
            )),
            log_line
        )
    } else {
        write!(w, "{log_line}")
    }
}

fn file_formatter(
    w: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record,
) -> std::io::Result<()> {
    write!(
        w,
        "[{}] [{:>5}] {}",
        now.now().format("%Y-%m-%d %H:%M:%S%.6f"),
        record.level(),
        record.args()
    )
}

pub fn log_file_path() -> PathBuf {
    let config = GLOBAL_SETTINGS.get().unwrap();
    let mut log_path = PathBuf::from(&ARGS.logs.as_ref().unwrap_or(&config.logs));

    if !log_path.is_absolute() {
        log_path = env::current_dir().unwrap().join(log_path);
    }

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
        .map(str::trim)
        .collect::<Vec<&str>>();

    let mut message = Message::builder()
        .from(config.sender_addr.parse()?)
        .subject(&config.subject)
        .header(header::ContentType::TEXT_PLAIN);

    for r in recipient {
        message = message.to(r.parse()?);
    }

    let mail = message.body(msg)?;
    let transporter = if config.starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_server)?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_server)?
    };

    let credentials = Credentials::new(config.sender_addr.clone(), config.sender_pass.clone());
    let mailer = transporter.credentials(credentials).build();

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
                    let interval = round_to_nearest_ten(counter as i64);
                    let mut q_lock = queue.lock().unwrap_or_else(|poisoned| {
                        error!("Queue mutex was poisoned");

                        poisoned.into_inner()
                    });

                    let expire = round_to_nearest_ten(q_lock.config.interval.max(30));

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
    let log_level = match ARGS.log_level.as_deref().map(str::to_lowercase).as_deref() {
        Some("debug") => LevelFilter::Debug,
        Some("error") => LevelFilter::Error,
        Some("info") => LevelFilter::Info,
        Some("trace") => LevelFilter::Trace,
        Some("warn") => LevelFilter::Warn,
        Some("off") => LevelFilter::Off,
        _ => LevelFilter::Debug,
    };

    mail_queue(mail_queues.clone());

    // Build the initial log specification
    let mut builder = LogSpecification::builder();
    builder
        .default(log_level)
        .module("actix", LevelFilter::Info)
        .module("actix_files", LevelFilter::Info)
        .module("actix_web", LevelFilter::Info)
        .module("actix_web_service", LevelFilter::Error)
        .module("hyper", LevelFilter::Error)
        .module("flexi_logger", LevelFilter::Error)
        .module("libc", LevelFilter::Error)
        .module("log", LevelFilter::Error)
        .module("mio", LevelFilter::Error)
        .module("neli", LevelFilter::Error)
        .module("reqwest", LevelFilter::Error)
        .module("rpc", LevelFilter::Error)
        .module("rustls", LevelFilter::Error)
        .module("serial_test", LevelFilter::Error)
        .module("sqlx", LevelFilter::Error)
        .module("tokio", LevelFilter::Error);

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
        info!("<bright black>[Server]</> {}", line.replace("[info] ", ""));
    } else if line.contains("[warning]")
        && (level.to_lowercase() == "warning" || level.to_lowercase() == "info")
    {
        warn!(
            "<bright black>[Server]</> {}",
            line.replace("[warning] ", "")
        );
    } else if line.contains("[error]")
        && !line.contains("Input/output error")
        && !line.contains("Broken pipe")
    {
        error!("<bright black>[Server]</> {}", line.replace("[error] ", ""));
    } else if line.contains("[fatal]") {
        error!("<bright black>[Server]</> {}", line.replace("[fatal] ", ""));
    }
}
