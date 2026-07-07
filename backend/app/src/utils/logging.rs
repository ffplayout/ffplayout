use std::{
    collections::{HashMap, hash_map},
    env, fmt,
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Instant,
};

use axum::{
    body::{Body, HttpBody},
    http::{
        Request, Response,
        header::{CONTENT_LENGTH, REFERER, USER_AGENT},
    },
    middleware::Next,
};
use chrono::{DateTime, FixedOffset};
use chrono_tz::Tz;
use flexi_logger::{
    Age, Cleanup, Criterion, DeferredNow, FileSpec, Level, LogSpecification, Logger, Naming,
    WriteMode,
    writers::{FileLogWriter, LogWriter},
};
use log::{kv::Value, *};
use real::RealIp;
use regex::{Captures, Regex};
use tokio::sync::Mutex;

use crate::{
    ARGS,
    db::GLOBAL_SETTINGS,
    utils::{
        mail::{MailQueue, mail_queue},
        time_machine::time_now,
    },
};

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.6f%:z";

#[derive(Debug)]
pub enum Target {
    Console,
    All,
}

impl Target {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Console if ARGS.log_to_console => "{_Default}",
            Self::Console => "{console}",
            Self::All if ARGS.log_to_console => "{_Default}",
            Self::All => "{console,_Default}",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
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
            .map_err(|e| io::Error::other(e.to_string()))?;

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

    async fn push_mail_async(
        mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
        id: i32,
        level: Level,
        now: String,
        msg: String,
    ) {
        let mut queues_guard = mail_queues.lock().await;

        for queue_arc in queues_guard.iter_mut() {
            let mut queue = queue_arc.lock().await;
            if push_mail_line(&mut queue, id, level, &now, &msg) {
                break;
            }
        }
    }

    fn push_mail_blocking(
        mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
        id: i32,
        level: Level,
        now: String,
        msg: String,
    ) {
        let mut queues_guard = mail_queues.blocking_lock();

        for queue_arc in queues_guard.iter_mut() {
            let mut queue = queue_arc.blocking_lock();
            if push_mail_line(&mut queue, id, level, &now, &msg) {
                break;
            }
        }
    }
}

fn push_mail_line(queue: &mut MailQueue, id: i32, level: Level, now: &str, msg: &str) -> bool {
    if queue.id != id || !queue.level_eq(level) || queue.raw_lines.contains(&msg.to_string()) {
        return false;
    }

    queue.push_raw(msg.to_string());
    queue.push(format!("[{now}] [{:>5}] {}", level, msg));

    if queue.raw_lines.len() > 1000 {
        let last = queue.raw_lines.pop().unwrap();
        queue.clear_raw();
        queue.push_raw(last);
    }

    true
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
        let now = now.now().format("%Y-%m-%d %H:%M:%S").to_string();
        let msg = strip_tags(&message);

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(Self::push_mail_async(mail_queues, id, level, now, msg));
        } else {
            Self::push_mail_blocking(mail_queues, id, level, now, msg);
        }

        Ok(())
    }
    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct LogDefault {
    file: Box<dyn LogWriter>,
    mail: LogMailer,
}

impl LogDefault {
    pub fn new(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) -> Self {
        Self {
            file: Box::new(MultiFileLogger::new(log_file_path())),
            mail: LogMailer::new(mail_queues),
        }
    }
}

impl LogWriter for LogDefault {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        self.file.write(now, record)?;
        self.mail.write(now, record)
    }

    fn flush(&self) -> std::io::Result<()> {
        self.file.flush()?;
        self.mail.flush()
    }
}

fn strip_tags(input: &str) -> String {
    let re = Regex::new(r"<[^>]*>").unwrap();
    re.replace_all(input, "").to_string()
}

fn format_level(record: &Record) -> String {
    match record.level() {
        Level::Trace => format!(
            "<span class=\"level-trace\">[TRACE]</span> {}:{} {}",
            record.file().unwrap_or_default(),
            record.line().unwrap_or_default(),
            record.args()
        ),
        Level::Debug => format!(
            "<span class=\"level-debug\">[DEBUG]</span> {}",
            record.args()
        ),
        Level::Info => format!(
            "<span class=\"level-info\">[ INFO]</span> {}",
            record.args()
        ),
        Level::Warn => format!(
            "<span class=\"level-warning\">[ WARN]</span> {}",
            record.args()
        ),
        Level::Error => format!(
            "<span class=\"level-error\">[ERROR]</span> {}",
            record.args()
        ),
    }
}

fn html_to_ansi(input: &str) -> String {
    let mut output = input.to_string();

    let replacements = vec![
        (
            r#"<span class="level-trace">([^<]+)</span>"#,
            "\x1b[93m$1\x1b[0m",
        ), // level bright yellow
        (
            r#"<span class="level-debug">([^<]+)</span>"#,
            "\x1b[94m$1\x1b[0m",
        ), // level bright blue
        (
            r#"<span class="level-info">([^<]+)</span>"#,
            "\x1b[92m$1\x1b[0m",
        ), // level green
        (
            r#"<span class="level-warning">([^<]+)</span>"#,
            "\x1b[33m$1\x1b[0m",
        ), // level yellow
        (
            r#"<span class="level-error">([^<]+)</span>"#,
            "\x1b[31m$1\x1b[0m",
        ), // level red
        // text and number formatting
        (
            r#"<span class="log-gray">([^<]+)</span>"#,
            "\x1b[90m$1\x1b[0m",
        ), // bright black
        (
            r#"<span class="log-addr">([^<]+)</span>"#,
            "\x1b[1;35m$1\x1b[0m",
        ), // bold magenta
        (
            r#"<span class="log-cmd">([^<]+)</span>"#,
            "\x1b[94m$1\x1b[0m",
        ), // bright blue
        (
            r#"<span class="log-number">([^<]+)</span>"#,
            "\x1b[33m$1\x1b[0m",
        ), // yellow
    ];

    for (pattern, replacement) in replacements {
        let re = Regex::new(pattern).unwrap();
        output = re.replace_all(&output, replacement).to_string();
    }

    output
}

pub fn remove_html(input: &str) -> String {
    let tag_re = Regex::new(r"<[^>]*>").unwrap();
    let space_re = Regex::new(r"\s{2,}").unwrap();

    let no_tags = tag_re.replace_all(input, "");
    let cleaned = space_re.replace_all(&no_tags, " ");

    cleaned.to_string()
}

pub fn timestamps_to_timezone(input: &str, target_tz: Tz) -> String {
    let re = Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?[+-]\d{2}:\d{2}").unwrap();

    re.replace_all(input, |caps: &Captures| {
        let ts_str = &caps[0];
        match ts_str.parse::<DateTime<FixedOffset>>() {
            Ok(original_dt) => {
                let converted = original_dt.with_timezone(&target_tz);

                converted.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
            }
            Err(_) => ts_str.to_string(),
        }
    })
    .to_string()
}

fn console_formatter(w: &mut dyn Write, now: &mut DeferredNow, record: &Record) -> io::Result<()> {
    let log_line = html_to_ansi(&format_level(record));

    if ARGS.log_timestamp {
        let time = if ARGS.fake_time.is_some() {
            time_now(&None).format(TIME_FORMAT)
        } else {
            now.now().format(TIME_FORMAT)
        };

        write!(
            w,
            "{} {}",
            html_to_ansi(&format!("<span class=\"log-gray\">[{time}]</span>")),
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
    let time = format!(
        "<span class=\"log-gray\">[{}]</span>",
        now.now().format(TIME_FORMAT)
    );
    let log_line = format_level(record);

    write!(w, "{time} {log_line}")
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

/// Initialize our logging, to have:
///
/// - default file and mail logger
/// - explicit console logger
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
        .module("cosmic_text", LevelFilter::Error)
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

    let mut logger = Logger::with(builder.build()).write_mode(WriteMode::Async);

    if ARGS.log_to_console {
        logger = logger.log_to_writer(Box::new(LogConsole));
    } else {
        logger = logger
            .log_to_writer(Box::new(LogDefault::new(mail_queues)))
            .add_writer("console", Box::new(LogConsole));
    }

    let logger = logger
        .start()
        .map_err(|e| io::Error::other(e.to_string()))?;

    Ok(logger)
}

/// Format ingest and HLS logging output
pub fn log_line(id: i32, line: &str, level: &str) {
    if line.contains("[info]") && level.to_lowercase() == "info" {
        info!(channel = id; "<span class=\"log-gray\">[Server]</span> {}", line.replace("[info] ", ""));
    } else if line.contains("[warning]")
        && (level.to_lowercase() == "warning" || level.to_lowercase() == "info")
    {
        warn!(
            channel = id;
            "<span class=\"log-gray\">[Server]</span> {}",
            line.replace("[warning] ", "")
        );
    } else if line.contains("[error]")
        && !line.contains("Input/output error")
        && !line.contains("Broken pipe")
    {
        error!(channel = id; "<span class=\"log-gray\">[Server]</span> {}", line.replace("[error] ", ""));
    } else if line.contains("[fatal]") {
        error!(channel = id; "<span class=\"log-gray\">[Server]</span> {}", line.replace("[fatal] ", ""));
    }
}

pub async fn log_middleware(real_ip: RealIp, req: Request<Body>, next: Next) -> Response<Body> {
    let start = Instant::now();
    let ip = real_ip.ip();

    let m = req.method().clone();
    let uri = req.uri().clone();
    let v = req.version();

    let r = req
        .headers()
        .get(REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();

    let a = req
        .headers()
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string();

    let res = next.run(req).await;

    let status = res.status().as_u16();
    let size = res
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            res.body()
                .size_hint()
                .exact()
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| "-".to_string());

    let l = start.elapsed().as_secs_f64();

    match status {
        500..=599 => {
            error!(target: Target::Console.as_str(), r#"{ip} "{m} {uri} {v:?}" {status} {size} "{r}" "{a}" {l:.6}"#);
        }
        401 | 403 | 429 => {
            warn!(target: Target::Console.as_str(), r#"{ip} "{m} {uri} {v:?}" {status} {size} "{r}" "{a}" {l:.6}"#);
        }
        _ => {
            info!(target: Target::Console.as_str(), r#"{ip} "{m} {uri} {v:?}" {status} {size} "{r}" "{a}" {l:.6}"#);
        }
    }

    res
}
