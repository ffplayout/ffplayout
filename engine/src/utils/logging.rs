use std::{
    collections::{HashMap, VecDeque, hash_map},
    env,
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, FixedOffset};
use chrono_tz::Tz;
use flexi_logger::{
    Age, Cleanup, Criterion, DeferredNow, FileSpec, Level, LogSpecification, Logger, Naming,
    WriteMode,
    writers::{FileLogWriter, LogWriter},
};

use log::{kv::Value, *};
use regex::{Captures, Regex};
use tokio::sync::Mutex;

use super::ARGS;

use crate::db::GLOBAL_SETTINGS;
use crate::utils::{
    ServiceError,
    config::FFMPEG_UNRECOVERABLE_ERRORS,
    mail::{MailQueue, mail_queue},
    time_machine::time_now,
};

use crate::player::controller::ProcessUnit;

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
        let msg = strip_tags(&message);

        tokio::spawn({
            async move {
                let mut queues_guard = mail_queues.lock().await;

                for queue_arc in queues_guard.iter_mut() {
                    let mut queue = queue_arc.lock().await;

                    if queue.id == id && queue.level_eq(level) && !queue.raw_lines.contains(&msg) {
                        queue.push_raw(msg.clone());
                        queue.push(format!("[{now}] [{:>5}] {}", level, msg));

                        if queue.raw_lines.len() > 1000 {
                            let last = queue.raw_lines.pop().unwrap();
                            queue.clear_raw();
                            queue.push_raw(last);
                        }

                        break;
                    }
                }
            }
        });

        Ok(())
    }
    fn flush(&self) -> std::io::Result<()> {
        Ok(())
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

fn file_logger() -> Box<dyn LogWriter> {
    if ARGS.log_to_console {
        Box::new(LogConsole)
    } else {
        Box::new(MultiFileLogger::new(log_file_path()))
    }
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
        // .format(console_formatter)
        .log_to_writer(Box::new(LogConsole))
        .add_writer("file", file_logger())
        .add_writer("mail", Box::new(LogMailer::new(mail_queues)))
        .start()
        .map_err(|e| io::Error::other(e.to_string()))?;

    Ok(logger)
}

/// Format ingest and HLS logging output
pub fn log_line(id: i32, line: &str, level: &str) {
    if line.contains("[info]") && level.to_lowercase() == "info" {
        info!(target: Target::file_mail(), channel = id; "<span class=\"log-gray\">[Server]</span> {}", line.replace("[info] ", ""));
    } else if line.contains("[warning]")
        && (level.to_lowercase() == "warning" || level.to_lowercase() == "info")
    {
        warn!(
            target: Target::file_mail(), channel = id;
            "<span class=\"log-gray\">[Server]</span> {}",
            line.replace("[warning] ", "")
        );
    } else if line.contains("[error]")
        && !line.contains("Input/output error")
        && !line.contains("Broken pipe")
    {
        error!(target: Target::file_mail(), channel = id; "<span class=\"log-gray\">[Server]</span> {}", line.replace("[error] ", ""));
    } else if line.contains("[fatal]") {
        error!(target: Target::file_mail(), channel = id; "<span class=\"log-gray\">[Server]</span> {}", line.replace("[fatal] ", ""));
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

/// Deduplicate log lines
/// This struct is used for ffmpeg logging because it can happen that too many repeated lines are written in a very short time.
pub struct LogDedup {
    repeat_counts: HashMap<String, usize>,
    seen_recently: VecDeque<String>,
    suffix: ProcessUnit,
    channel_id: i32,
}

impl LogDedup {
    pub fn new(suffix: ProcessUnit, channel_id: i32) -> Self {
        Self {
            repeat_counts: HashMap::new(),
            seen_recently: VecDeque::with_capacity(5),
            suffix,
            channel_id,
        }
    }

    pub fn log(&mut self, msg: &str) -> Result<(), ServiceError> {
        if self.seen_recently.contains(&msg.to_string()) {
            *self.repeat_counts.entry(msg.to_string()).or_insert(1) += 1;

            return Ok(());
        }

        for (msg, count) in self.repeat_counts.drain() {
            if count > 1 {
                let result = format!("{msg} (repeated <span class=\"log-number\">{count}x</span>)");
                stderr_log(&result, self.suffix, self.channel_id)?;
            }
        }

        stderr_log(msg, self.suffix, self.channel_id)?;

        if self.seen_recently.len() == 5 {
            self.seen_recently.pop_front();
        }
        self.seen_recently.push_back(msg.to_string());

        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), ServiceError> {
        for (msg, count) in self.repeat_counts.drain() {
            if count > 1 {
                let result = format!("{msg} (repeated <span class=\"log-number\">{count}x</span>)");
                stderr_log(&result, self.suffix, self.channel_id)?;
            }
        }

        Ok(())
    }
}

pub fn stderr_log(line: &str, suffix: ProcessUnit, channel_id: i32) -> Result<(), ServiceError> {
    if line.contains("[info]") {
        info!(target: Target::file_mail(), channel = channel_id;
            "<span class=\"log-gray\">[{suffix}]</span> {}",
            line.replace("[info] ", "")
        );
    } else if line.contains("[warning]") {
        warn!(target: Target::file_mail(), channel = channel_id;
            "<span class=\"log-gray\">[{suffix}]</span> {}",
            line.replace("[warning] ", "")
        );
    } else if line.contains("[error]") || line.contains("[fatal]") {
        error!(target: Target::file_mail(), channel = channel_id;
            "<span class=\"log-gray\">[{suffix}]</span> {}",
            line.replace("[error] ", "").replace("[fatal] ", "")
        );

        if FFMPEG_UNRECOVERABLE_ERRORS
            .iter()
            .any(|i| line.contains(*i))
            || (line.contains("No such file or directory")
                && !line.contains("failed to delete old segment"))
        {
            return Err(ServiceError::Conflict(
                "Hit unrecoverable error!".to_string(),
            ));
        }
    }

    Ok(())
}
