use std::{
    collections::{hash_map, HashMap},
    env,
    io::{self, ErrorKind, Write},
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use actix_web::rt::time::interval;
use flexi_logger::{
    writers::{FileLogWriter, LogWriter},
    Age, Cleanup, Criterion, DeferredNow, FileSpec, Level, LogSpecification, Logger, Naming,
    WriteMode,
};
use lettre::{
    message::header, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use log::{kv::Value, *};
use paris::formatter::colorize_string;
use regex::Regex;
use tokio::sync::Mutex;

use super::ARGS;

use crate::db::GLOBAL_SETTINGS;
use crate::utils::{
    config::Mail, errors::ProcessError, round_to_nearest_ten, time_machine::time_now,
};

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.6f%:z";

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

pub struct MultiFileLogger {
    log_path: PathBuf,
    writers: RwLock<HashMap<i32, Arc<FileLogWriter>>>,
}

impl MultiFileLogger {
    pub fn new(log_path: PathBuf) -> Self {
        Self {
            log_path,
            writers: RwLock::new(HashMap::new()),
        }
    }

    fn get_writer(&self, channel: i32) -> io::Result<Arc<FileLogWriter>> {
        // Lock the writers HashMap
        let mut writers = self.writers.write().unwrap();

        // Check if the writer already exists
        if let hash_map::Entry::Vacant(entry) = writers.entry(channel) {
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

            let arc_writer = Arc::new(writer);
            entry.insert(arc_writer.clone());
            Ok(arc_writer)
        } else {
            Ok(writers.get(&channel).unwrap().clone())
        }
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

        let writer = self.get_writer(channel)?;
        writer.write(now, record)
    }

    fn flush(&self) -> io::Result<()> {
        let writers = self.writers.read().unwrap();
        for writer in writers.values() {
            writer.flush()?;
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
                .and_then(|v| Value::to_i64(&v))
                .unwrap_or(0),
        )
        .unwrap_or(0);

        let message = record.args().to_string();
        let level = record.level();
        let mail_queues = self.mail_queues.clone();
        let now = now.now().format("%Y-%m-%d %H:%M:%S");

        tokio::spawn(async move {
            let mut queues = mail_queues.lock().await;

            for queue in queues.iter_mut() {
                let mut q_lock = queue.lock().await;

                let msg = strip_tags(&message);

                if q_lock.id == id && q_lock.level_eq(level) && !q_lock.raw_lines.contains(&msg) {
                    q_lock.push_raw(msg.clone());
                    q_lock.push(format!("[{now}] [{:>5}] {}", level, msg));

                    break;
                }

                if q_lock.raw_lines.len() > 1000 {
                    let last = q_lock.raw_lines.pop().unwrap();
                    q_lock.clear_raw();
                    q_lock.push_raw(last);
                }
            }
        });

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
    pub raw_lines: Vec<String>,
}

impl MailQueue {
    pub fn new(id: i32, config: Mail) -> Self {
        Self {
            id,
            config,
            lines: vec![],
            raw_lines: vec![],
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

    pub fn clear_raw(&mut self) {
        self.raw_lines.clear();
    }

    pub fn push(&mut self, line: String) {
        self.lines.push(line);
    }

    pub fn push_raw(&mut self, line: String) {
        self.raw_lines.push(line);
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
            time_now(&None).format(TIME_FORMAT)
        } else {
            now.now().format(TIME_FORMAT)
        };

        write!(
            w,
            "{} {}",
            colorize_string(format!("<bright black>[{time}]</>")),
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
        now.now().format(TIME_FORMAT),
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
        .from(config.smtp_user.parse()?)
        .subject(&config.subject)
        .header(header::ContentType::TEXT_PLAIN);

    for r in recipient {
        message = message.to(r.parse()?);
    }

    let mail = message.body(msg)?;
    let transporter = if config.smtp_starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_server)?
            .port(config.smtp_port)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_server)?.port(config.smtp_port)
    };

    let credentials = Credentials::new(config.smtp_user.clone(), config.smtp_password.clone());
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
                let mut queues = mail_queues.lock().await;

                // Process mail queues and send emails
                for queue in queues.iter_mut() {
                    let interval = round_to_nearest_ten(counter as i64);
                    let mut q_lock = queue.lock().await;

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
pub fn init_logging(
    mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> io::Result<flexi_logger::LoggerHandle> {
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

    let logger = Logger::with(builder.build())
        .write_mode(WriteMode::Async)
        .format(console_formatter)
        .log_to_stderr()
        .add_writer("file", file_logger())
        .add_writer("mail", Box::new(LogMailer::new(mail_queues)))
        .start()
        .map_err(|e| io::Error::new(ErrorKind::Other, e.to_string()))?;

    Ok(logger)
}

/// Format ingest and HLS logging output
pub fn log_line(id: i32, line: &str, level: &str) {
    if line.contains("[info]") && level.to_lowercase() == "info" {
        info!(target: Target::file_mail(), channel = id; "<bright black>[Server]</> {}", line.replace("[info] ", ""));
    } else if line.contains("[warning]")
        && (level.to_lowercase() == "warning" || level.to_lowercase() == "info")
    {
        warn!(
            target: Target::file_mail(), channel = id;
            "<bright black>[Server]</> {}",
            line.replace("[warning] ", "")
        );
    } else if line.contains("[error]")
        && !line.contains("Input/output error")
        && !line.contains("Broken pipe")
    {
        error!(target: Target::file_mail(), channel = id; "<bright black>[Server]</> {}", line.replace("[error] ", ""));
    } else if line.contains("[fatal]") {
        error!(target: Target::file_mail(), channel = id; "<bright black>[Server]</> {}", line.replace("[fatal] ", ""));
    }
}

pub fn fmt_cmd(cmd: &[String]) -> String {
    let mut formatted_cmd = Vec::new();
    let mut quote_next = false;

    for (i, arg) in cmd.iter().enumerate() {
        if quote_next
            || (i == cmd.len() - 1)
            || ["ts", "m3u8"].contains(
                &arg.rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_lowercase()
                    .as_str(),
            )
        {
            formatted_cmd.push(format!("\"{}\"", arg));
            quote_next = false;
        } else {
            formatted_cmd.push(arg.to_string());
            if [
                "-i",
                "-filter_complex",
                "-map",
                "-metadata",
                "-var_stream_map",
            ]
            .contains(&arg.as_str())
            {
                quote_next = true;
            }
        }
    }

    formatted_cmd.join(" ")
}
