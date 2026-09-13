use std::{
    collections::{HashMap, hash_map},
    env, fmt,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex as StdMutex, RwLock},
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
        notification::LogNotifier,
        time_machine::time_now,
    },
};

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.6f%:z";

/// Log an error that identifies a fatal condition.
#[macro_export]
macro_rules! fatal {
    (target: $target:expr, $($key:ident = $value:expr),+; $($arg:tt)+) => {
        log::log!(
            target: $target,
            log::Level::Error,
            $($key = $value,)+ fatal = true;
            $($arg)+
        )
    };

    (target: $target:expr, $($arg:tt)+) => {
        log::log!(
            target: $target,
            log::Level::Error,
            fatal = true;
            $($arg)+
        )
    };

    ($($key:ident = $value:expr),+; $($arg:tt)+) => {
        log::error!(
            $($key = $value,)+ fatal = true;
            $($arg)+
        )
    };

    ($($arg:tt)+) => {
        log::error!(
            fatal = true;
            $($arg)+
        )
    };
}

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

#[derive(Clone, Default)]
pub struct LogConsole {
    state: Arc<StdMutex<ConsoleState>>,
    notifier: Option<Arc<LogNotifier>>,
}

impl LogConsole {
    pub fn with_notifier(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) -> Self {
        Self {
            state: Arc::default(),
            notifier: Some(Arc::new(LogNotifier::new(mail_queues))),
        }
    }
}

#[derive(Default)]
struct ConsoleState {
    previous_bench_lines: usize,
}

impl LogWriter for LogConsole {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        let stderr = io::stderr();
        let is_terminal = stderr.is_terminal();
        {
            let mut stderr = stderr.lock();
            self.write_to(&mut stderr, is_terminal, now, record, console_formatter)?;
        }
        if let Some(notifier) = &self.notifier {
            notifier.write(now, record)?;
        }
        Ok(())
    }

    fn flush(&self) -> std::io::Result<()> {
        io::stderr().flush()?;
        if let Some(notifier) = &self.notifier {
            notifier.flush()?;
        }
        Ok(())
    }
}

impl LogConsole {
    fn write_to(
        &self,
        output: &mut dyn Write,
        is_terminal: bool,
        now: &mut DeferredNow,
        record: &Record<'_>,
        format: flexi_logger::FormatFunction,
    ) -> io::Result<()> {
        let message = record.args().to_string();
        let bench_lines = cpu_bench_line_count(&message);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if is_terminal && bench_lines.is_some() && state.previous_bench_lines > 0 {
            // The previous table is still the last terminal output, so it is
            // safe to move back to its first line and redraw it in place.
            write!(output, "\x1b[{}A\r\x1b[J", state.previous_bench_lines)?;
        }

        format(output, now, record)?;
        if !message.ends_with('\n') {
            writeln!(output)?;
        }

        state.previous_bench_lines = if is_terminal {
            bench_lines.unwrap_or_default()
        } else {
            0
        };

        Ok(())
    }
}

fn cpu_bench_line_count(message: &str) -> Option<usize> {
    (message.starts_with("[CPU Bench]")
        || message.starts_with("<span class=\"log-gray\">[CPU Bench]</span>"))
    .then(|| message.lines().count().max(1))
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
        let Some(channel) = record_channel(record) else {
            return Ok(());
        };

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
    console: LogConsole,
    mail: LogMailer,
    notifier: LogNotifier,
}

impl LogDefault {
    pub fn new(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>, console: LogConsole) -> Self {
        Self {
            file: Box::new(MultiFileLogger::new(log_file_path())),
            console,
            mail: LogMailer::new(mail_queues.clone()),
            notifier: LogNotifier::new(mail_queues),
        }
    }
}

impl LogWriter for LogDefault {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        if record_channel(record).is_none() {
            // The default writer normally targets a channel-specific file. A
            // record without a valid channel is process-wide and belongs on
            // the console instead; LogConsole also converts embedded HTML
            // styling to ANSI terminal sequences.
            return if explicitly_targets_console(record) {
                // An explicitly addressed console writer already receives it.
                Ok(())
            } else {
                self.console.write(now, record)
            };
        }

        self.file.write(now, record)?;
        self.mail.write(now, record)?;
        self.notifier.write(now, record)
    }

    fn flush(&self) -> std::io::Result<()> {
        self.file.flush()?;
        self.console.flush()?;
        self.mail.flush()?;
        self.notifier.flush()
    }
}

fn record_channel(record: &Record<'_>) -> Option<i32> {
    record
        .key_values()
        .get("channel".into())
        .and_then(|value| Value::to_i64(&value))
        .and_then(|channel| i32::try_from(channel).ok())
        .filter(|channel| *channel > 0)
}

fn explicitly_targets_console(record: &Record<'_>) -> bool {
    record
        .target()
        .strip_prefix('{')
        .and_then(|target| target.strip_suffix('}'))
        .is_some_and(|writers| writers.split(',').any(|writer| writer.trim() == "console"))
}

pub(crate) fn strip_tags(input: &str) -> String {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());
    TAG.replace_all(input, "").into_owned()
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
        Level::Error if is_fatal(record) => format!(
            "<span class=\"level-fatal\">[FATAL]</span> {}",
            record.args()
        ),
        Level::Error => format!(
            "<span class=\"level-error\">[ERROR]</span> {}",
            record.args()
        ),
    }
}

pub(crate) fn is_fatal(record: &Record) -> bool {
    record
        .key_values()
        .get("fatal".into())
        .and_then(|value| Value::to_bool(&value))
        .unwrap_or(false)
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
        (
            r#"<span class="level-fatal">([^<]+)</span>"#,
            "\x1b[1;91m$1\x1b[0m",
        ), // level bold bright red
        // text and number formatting
        (
            r#"<span class="log-gray">([^<]+)</span>"#,
            "\x1b[90m$1\x1b[0m",
        ), // bright black
        (
            r#"<span class="log-bold">([^<]+)</span>"#,
            "\x1b[1m$1\x1b[0m",
        ), // bold
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
        .module("naga", LevelFilter::Error)
        .module("pixels", LevelFilter::Error)
        .module("reqwest", LevelFilter::Error)
        .module("rpc", LevelFilter::Error)
        .module("rustls", LevelFilter::Error)
        .module("serial_test", LevelFilter::Error)
        .module("sctk", LevelFilter::Error)
        .module("sqlx", LevelFilter::Error)
        .module("tokio", LevelFilter::Error)
        // The tracing crate mirrors unannotated spans (such as winit's window
        // operations) into the log facade under this target.
        .module("tracing::span", LevelFilter::Error)
        .module("wgpu", LevelFilter::Error)
        .module("winit", LevelFilter::Error);

    let mut logger = Logger::with(builder.build()).write_mode(WriteMode::Async);

    if ARGS.log_to_console {
        logger = logger.log_to_writer(Box::new(LogConsole::with_notifier(mail_queues)));
    } else {
        let console = LogConsole::default();
        logger = logger
            .log_to_writer(Box::new(LogDefault::new(mail_queues, console.clone())))
            .add_writer("console", Box::new(console));
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
        fatal!(channel = id; "<span class=\"log-gray\">[Server]</span> {}", line.replace("[fatal] ", ""));
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

#[cfg(test)]
mod tests {
    use log::{Level, Record};

    use super::{
        LogConsole, cpu_bench_line_count, explicitly_targets_console, format_level, html_to_ansi,
        record_channel,
    };

    #[test]
    fn shared_console_preserves_messages_between_benchmark_updates() {
        let format: flexi_logger::FormatFunction =
            |output, _, record| write!(output, "{}", html_to_ansi(&format_level(record)));
        let console = LogConsole::default();
        let fallback = console.clone();
        let mut output = Vec::new();
        let mut now = flexi_logger::DeferredNow::new();
        let bench = Record::builder()
            .args(format_args!("[CPU Bench]\n    decode"))
            .level(Level::Info)
            .build();
        let warning = Record::builder()
            .args(format_args!(
                "<span class=\"log-addr\">connection lost</span>"
            ))
            .level(Level::Warn)
            .build();

        for (table_writer, message_writer) in [(&console, &fallback), (&fallback, &console)] {
            table_writer
                .write_to(&mut output, true, &mut now, &bench, format)
                .unwrap();
            message_writer
                .write_to(&mut output, true, &mut now, &warning, format)
                .unwrap();
            output.clear();
            table_writer
                .write_to(&mut output, true, &mut now, &bench, format)
                .unwrap();
            assert!(!String::from_utf8_lossy(&output).contains("\x1b[J"));

            output.clear();
            message_writer
                .write_to(&mut output, true, &mut now, &bench, format)
                .unwrap();
            assert!(String::from_utf8_lossy(&output).starts_with("\x1b[2A\r\x1b[J"));
        }
        output.clear();
        fallback
            .write_to(&mut output, true, &mut now, &warning, format)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\x1b[1;35mconnection lost\x1b[0m"));
        assert!(output.ends_with('\n'));
    }

    #[test]
    fn only_positive_channel_ids_select_a_file_log() {
        let missing = Record::builder()
            .level(Level::Info)
            .args(format_args!("missing channel"))
            .build();
        assert_eq!(record_channel(&missing), None);

        for (value, expected) in [(0_i64, None), (-1, None), (7, Some(7))] {
            let key_values = ("channel", value);
            let record = Record::builder()
                .level(Level::Info)
                .key_values(&key_values)
                .args(format_args!("channel test"))
                .build();
            assert_eq!(record_channel(&record), expected);
        }
    }

    #[test]
    fn console_output_converts_html_formatting_to_ansi() {
        let output = html_to_ansi(
            "<span class=\"level-info\">[ INFO]</span> \
             <span class=\"log-addr\">example</span>",
        );

        assert!(output.contains("\x1b[92m[ INFO]\x1b[0m"));
        assert!(output.contains("\x1b[1;35mexample\x1b[0m"));
        assert!(!output.contains("<span"));
    }

    #[test]
    fn detects_only_explicit_console_writer_targets() {
        let routed = Record::builder()
            .target("{console,_Default}")
            .level(Level::Info)
            .args(format_args!("routed"))
            .build();
        let module = Record::builder()
            .target("ffplayout::console")
            .level(Level::Info)
            .args(format_args!("module"))
            .build();

        assert!(explicitly_targets_console(&routed));
        assert!(!explicitly_targets_console(&module));
    }

    #[test]
    fn counts_cpu_bench_lines_only() {
        assert_eq!(cpu_bench_line_count("[CPU Bench]\n    decode"), Some(2));
        assert_eq!(
            cpu_bench_line_count("<span class=\"log-gray\">[CPU Bench]</span>\n    decode"),
            Some(2)
        );
        assert_eq!(cpu_bench_line_count("regular log line"), None);
    }

    /// Run with `cargo test -p ffplayout displays_console_log_levels -- --ignored --nocapture`.
    #[test]
    #[ignore = "prints every log level using the console formatter"]
    fn displays_console_log_levels() {
        for level in [
            Level::Trace,
            Level::Debug,
            Level::Info,
            Level::Warn,
            Level::Error,
        ] {
            let args = format_args!("example {level} message");
            let record = Record::builder().level(level).args(args).build();
            eprintln!("{}", html_to_ansi(&format_level(&record)));
        }

        let fatal_values = ("fatal", true);
        let fatal_record = Record::builder()
            .level(Level::Error)
            .key_values(&fatal_values)
            .args(format_args!("example fatal message"))
            .build();
        eprintln!("{}", html_to_ansi(&format_level(&fatal_record)));
    }
}
