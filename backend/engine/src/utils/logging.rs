use std::{
    cell::{Cell, RefCell},
    ffi::CStr,
    os::raw::{c_char, c_int, c_void},
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicI32, Ordering},
    },
};
#[cfg(any(ffplayout_srt_linked, test))]
use std::{
    mem,
    time::{Duration, Instant},
};

use ffmpeg_next::{ffi, util::log::Level as FfmpegLevel};
use log::{debug, error, info, trace, warn};
use regex::Regex;

use super::config::LogLevel;

const FFMPEG_LOG_TARGET: &str = "ffmpeg";
#[cfg(ffplayout_srt_linked)]
const SRT_LOG_TARGET: &str = "srt";
#[cfg(any(ffplayout_srt_linked, test))]
const SRT_LOG_REPEAT_INTERVAL: Duration = Duration::from_secs(10);
const SKIPPED_FFMPEG_LOG_MESSAGES: &[&str] = &[
    r"Opening '.*' for reading",
    r"Opening '.*' for writing",
    r"Could not update timestamps for skipped samples",
    r"ac-tex damaged",
    r"corrupt decoded frame in stream",
    r"corrupt input packet in stream",
    r"end mismatch left",
    r"Invalid mb type in I-frame at",
    r"Packet corrupt",
    r"Referenced QT chapter track not found",
    r"skipped MB in I-frame at",
    r"Thread message queue blocking",
    r"Warning MVs not available",
    r"frame size not set",
    r"Error parsing Opus packet header.",
];
const LOG_DEDUP_FLUSH_THRESHOLD: usize = 100;
const LOG_DEDUP_WINDOW: u64 = 6;

thread_local! {
    static UNEXPECTED_RTMP_STREAM: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static INGEST_LOG_CONTEXT: Cell<Option<IngestLogContext>> = const { Cell::new(None) };
    static INGEST_INTERRUPTED: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IngestLogContext {
    channel_id: i32,
    listener_id: i32,
}

static FFMPEG_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);
static INGEST_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);
static CHANNEL_ID: AtomicI32 = AtomicI32::new(0);
static LOG_DEDUP: Mutex<LogDedup> = Mutex::new(LogDedup::new());
static SKIPPED_FFMPEG_LOG_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
static USER_SKIPPED_FFMPEG_LOG_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
#[cfg(ffplayout_srt_linked)]
static SRT_LOG_DEDUP: Mutex<SrtLogDedup> = Mutex::new(SrtLogDedup::new());
static SRT_LOG_INSTALLED: OnceLock<bool> = OnceLock::new();

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
type FfmpegVaList = *mut ffi::__va_list_tag;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
type FfmpegVaList = ffi::va_list;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
type FfmpegVaList = *mut ffi::__va_list_tag;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
type FfmpegVaList = ffi::va_list;
#[cfg(target_os = "windows")]
type FfmpegVaList = ffi::va_list;

pub(crate) fn init(
    ffmpeg_level: LogLevel,
    ingest_level: LogLevel,
    ignore_lines: &[String],
    channel_id: Option<i32>,
) {
    let ffmpeg_level = ffmpeg_level.as_ffmpeg_level();
    let ingest_level = ingest_level.as_ffmpeg_level();
    FFMPEG_LOG_LEVEL.store(level_value(ffmpeg_level), Ordering::Relaxed);
    INGEST_LOG_LEVEL.store(level_value(ingest_level), Ordering::Relaxed);
    CHANNEL_ID.store(channel_id.unwrap_or(0), Ordering::Relaxed);
    *USER_SKIPPED_FFMPEG_LOG_LINES
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = ignore_lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    ffmpeg_next::util::log::set_level(max_level(ffmpeg_level, ingest_level));
    set_log_callback();
    SRT_LOG_INSTALLED.get_or_init(install_srt_log_callback);
}

#[cfg(ffplayout_srt_linked)]
type SrtLogHandler =
    unsafe extern "C" fn(*mut c_void, c_int, *const c_char, c_int, *const c_char, *const c_char);

#[cfg(ffplayout_srt_linked)]
unsafe extern "C" {
    fn srt_setlogflags(flags: c_int);
    fn srt_setloghandler(opaque: *mut c_void, handler: Option<SrtLogHandler>);
}

#[cfg(ffplayout_srt_linked)]
fn install_srt_log_callback() -> bool {
    unsafe {
        // The callback uses our own timestamp and severity. These flag values
        // are part of libsrt's public logging_api.h.
        srt_setlogflags(1 | 2 | 4 | 8);
        srt_setloghandler(std::ptr::null_mut(), Some(srt_log_callback));
    }

    true
}

#[cfg(not(ffplayout_srt_linked))]
fn install_srt_log_callback() -> bool {
    false
}

pub(crate) fn srt_log_callback_installed() -> bool {
    *SRT_LOG_INSTALLED.get_or_init(install_srt_log_callback)
}

#[cfg(ffplayout_srt_linked)]
unsafe extern "C" fn srt_log_callback(
    _opaque: *mut c_void,
    level: c_int,
    _file: *const c_char,
    _line: c_int,
    _area: *const c_char,
    message: *const c_char,
) {
    if message.is_null() {
        return;
    }

    // No Rust panic may unwind through the C callback on a libsrt worker.
    let _ = std::panic::catch_unwind(|| {
        let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
        let message = message.trim();

        if !message.is_empty() {
            log_srt_line(level, message);
        }
    });
}

#[cfg(ffplayout_srt_linked)]
fn log_srt_line(level: c_int, message: &str) {
    let line = SRT_LOG_DEDUP
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .record(message, Instant::now());
    let Some(line) = line else {
        return;
    };

    // SRT's logging callback runs on its own worker threads; thread-local
    // FFmpeg channel context is not reliable here. Use channel 0 (console).
    if level <= 3 {
        error!(target: SRT_LOG_TARGET, channel = 0; "<span class=\"log-gray\">[srt]</span> {line}");
    } else if level <= 4 {
        warn!(target: SRT_LOG_TARGET, channel = 0; "<span class=\"log-gray\">[srt]</span> {line}");
    } else if level <= 6 {
        info!(target: SRT_LOG_TARGET, channel = 0; "<span class=\"log-gray\">[srt]</span> {line}");
    } else {
        debug!(target: SRT_LOG_TARGET, channel = 0; "<span class=\"log-gray\">[srt]</span> {line}");
    }
}

#[cfg(any(ffplayout_srt_linked, test))]
struct SrtLogDedup {
    receive_buffer_warnings: Vec<SrtRepeat>,
}

#[cfg(any(ffplayout_srt_linked, test))]
struct SrtRepeat {
    socket_id: Option<String>,
    last_emit: Instant,
    suppressed: u64,
}

#[cfg(any(ffplayout_srt_linked, test))]
impl SrtLogDedup {
    const fn new() -> Self {
        Self {
            receive_buffer_warnings: Vec::new(),
        }
    }

    fn record(&mut self, message: &str, now: Instant) -> Option<String> {
        if !message.contains("No room to store incoming packet") {
            return Some(message.to_string());
        }

        let socket_id = message
            .split_once('@')
            .and_then(|(_, rest)| rest.split_once(':'))
            .map(|(id, _)| id)
            .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()));
        if let Some(entry) = self
            .receive_buffer_warnings
            .iter_mut()
            .find(|entry| entry.socket_id.as_deref() == socket_id)
        {
            if now.saturating_duration_since(entry.last_emit) < SRT_LOG_REPEAT_INTERVAL {
                entry.suppressed += 1;

                return None;
            }

            let suppressed = mem::take(&mut entry.suppressed);
            entry.last_emit = now;

            return if suppressed == 0 {
                Some(message.to_string())
            } else {
                Some(format!(
                    "SRT receive buffer full ({suppressed} similar warnings suppressed in the last 10 s); latest: {message}"
                ))
            };
        }

        // Keep this process-wide table bounded even if many sockets churn.
        if self.receive_buffer_warnings.len() >= 32 {
            self.receive_buffer_warnings.remove(0);
        }
        self.receive_buffer_warnings.push(SrtRepeat {
            socket_id: socket_id.map(str::to_string),
            last_emit: now,
            suppressed: 0,
        });

        Some(message.to_string())
    }
}

fn set_log_callback() {
    unsafe {
        ffi::av_log_set_callback(Some(log_callback));
    }
}

pub(crate) fn with_ingest_logs<T>(
    channel_id: Option<i32>,
    listener_id: i32,
    operation: impl FnOnce() -> T,
) -> T {
    let context = IngestLogContext {
        channel_id: channel_id.unwrap_or_default(),
        listener_id,
    };
    let previous = INGEST_LOG_CONTEXT.with(|current| current.replace(Some(context)));
    let previous_interrupted = INGEST_INTERRUPTED.with(|interrupted| interrupted.replace(false));
    let result = operation();
    INGEST_INTERRUPTED.with(|interrupted| interrupted.set(previous_interrupted));
    INGEST_LOG_CONTEXT.with(|current| current.set(previous));

    result
}

/// Marks the current ingest operation as intentionally interrupted. FFmpeg
/// often emits a connection error while its I/O interrupt callback unwinds;
/// that message is expected during shutdown and should not be logged as an
/// operational failure.
pub(crate) fn mark_ingest_interrupted() {
    INGEST_INTERRUPTED.with(|interrupted| interrupted.set(true));
}

pub(crate) fn clear_unexpected_rtmp_stream() {
    UNEXPECTED_RTMP_STREAM.with(|stream| {
        *stream.borrow_mut() = None;
    });
}

pub(crate) fn take_unexpected_rtmp_stream() -> Option<(String, String)> {
    UNEXPECTED_RTMP_STREAM.with(|stream| stream.borrow_mut().take())
}

fn configured_level(context: Option<IngestLogContext>) -> c_int {
    if context.is_some() {
        INGEST_LOG_LEVEL.load(Ordering::Relaxed)
    } else {
        FFMPEG_LOG_LEVEL.load(Ordering::Relaxed)
    }
}

fn max_level(left: FfmpegLevel, right: FfmpegLevel) -> FfmpegLevel {
    if level_value(left) >= level_value(right) {
        left
    } else {
        right
    }
}

fn level_value(level: FfmpegLevel) -> c_int {
    c_int::from(level)
}

unsafe extern "C" fn log_callback(
    avcl: *mut c_void,
    level: c_int,
    fmt: *const c_char,
    vl: FfmpegVaList,
) {
    let context = INGEST_LOG_CONTEXT.with(Cell::get);

    if level > unsafe { ffi::av_log_get_level() } || level > configured_level(context) {
        return;
    }

    let mut line = [0 as c_char; 4096];
    let mut print_prefix = 1;
    let result = unsafe {
        ffi::av_log_format_line2(
            avcl,
            level,
            fmt,
            vl,
            line.as_mut_ptr(),
            line.len() as c_int,
            &mut print_prefix,
        )
    };

    if result < 0 {
        return;
    }

    let message = unsafe { CStr::from_ptr(line.as_ptr()) }
        .to_string_lossy()
        .trim()
        .to_owned();
    if message.is_empty() {
        return;
    }

    for line in message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !should_skip_ffmpeg_log(line))
    {
        log_line(level, line, context);
    }
}

fn should_skip_ffmpeg_log(message: &str) -> bool {
    INGEST_INTERRUPTED.with(Cell::get)
        || skipped_ffmpeg_log_patterns()
            .iter()
            .any(|pattern| pattern.is_match(message))
        || USER_SKIPPED_FFMPEG_LOG_LINES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|line| message.contains(line))
}

fn skipped_ffmpeg_log_patterns() -> &'static [Regex] {
    SKIPPED_FFMPEG_LOG_PATTERNS.get_or_init(|| {
        SKIPPED_FFMPEG_LOG_MESSAGES
            .iter()
            .map(|pattern| {
                Regex::new(pattern).unwrap_or_else(|error| {
                    panic!("invalid skipped FFmpeg log pattern {pattern:?}: {error}")
                })
            })
            .collect()
    })
}

fn log_line(level: c_int, message: &str, context: Option<IngestLogContext>) {
    remember_unexpected_rtmp_stream(message);

    let mut dedup = LOG_DEDUP.lock().unwrap_or_else(PoisonError::into_inner);

    for repeated in dedup.push_with_context(level, message, context) {
        write_log_line(
            repeated.level,
            repeated.channel_id,
            repeated.listener_id,
            &repeated.message,
        );
    }
}

fn write_log_line(level: c_int, channel_id: i32, listener_id: Option<i32>, message: &str) {
    let ingest_tag = ffmpeg_ingest_tag(listener_id);

    if level <= ffi::AV_LOG_ERROR {
        error!(target: FFMPEG_LOG_TARGET, channel = channel_id; "<span class=\"log-gray\">[ffmpeg]</span>{ingest_tag} {message}");
    } else if level <= ffi::AV_LOG_WARNING {
        warn!(target: FFMPEG_LOG_TARGET, channel = channel_id; "<span class=\"log-gray\">[ffmpeg]</span>{ingest_tag} {message}");
    } else if level <= ffi::AV_LOG_INFO {
        info!(target: FFMPEG_LOG_TARGET, channel = channel_id; "<span class=\"log-gray\">[ffmpeg]</span>{ingest_tag} {message}");
    } else if level <= ffi::AV_LOG_DEBUG {
        debug!(target: FFMPEG_LOG_TARGET, channel = channel_id; "<span class=\"log-gray\">[ffmpeg]</span>{ingest_tag} {message}");
    } else {
        trace!(target: FFMPEG_LOG_TARGET, channel = channel_id; "<span class=\"log-gray\">[ffmpeg]</span>{ingest_tag} {message}");
    }
}

fn ffmpeg_ingest_tag(listener_id: Option<i32>) -> String {
    listener_id
        .map(|id| format!(" <span class=\"log-gray\">[ingest #{id}]</span>"))
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DedupLine {
    level: c_int,
    channel_id: i32,
    listener_id: Option<i32>,
    message: String,
}

#[derive(Debug)]
struct LogDedup {
    entries: Vec<DedupEntry>,
    sequence: u64,
}

#[derive(Debug)]
struct DedupEntry {
    key: DedupKey,
    summary_message: String,
    last_seen: u64,
    repeat_count: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct DedupKey {
    level: c_int,
    channel_id: i32,
    listener_id: Option<i32>,
    fingerprint: String,
}

impl LogDedup {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            sequence: 0,
        }
    }

    #[cfg(test)]
    fn push(&mut self, level: c_int, message: &str) -> Vec<DedupLine> {
        self.push_with_context(level, message, None)
    }

    fn push_with_context(
        &mut self,
        level: c_int,
        message: &str,
        context: Option<IngestLogContext>,
    ) -> Vec<DedupLine> {
        self.sequence = self.sequence.wrapping_add(1);
        let channel_id =
            context.map_or_else(|| CHANNEL_ID.load(Ordering::Relaxed), |ctx| ctx.channel_id);
        let listener_id = context.map(|ctx| ctx.listener_id);
        let (fingerprint, summary_message) = ffmpeg_log_fingerprint(message);
        let key = DedupKey {
            level,
            channel_id,
            listener_id,
            fingerprint,
        };
        let line = DedupLine {
            level,
            channel_id,
            listener_id,
            message: message.to_string(),
        };
        let mut lines = self.drain_expired();

        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
            entry.last_seen = self.sequence;
            entry.repeat_count += 1;

            if entry.repeat_count >= LOG_DEDUP_FLUSH_THRESHOLD {
                if let Some(repeated) = entry.repeated_line() {
                    lines.push(repeated);
                }

                entry.repeat_count = 0;
            }

            return lines;
        }

        self.entries.push(DedupEntry {
            key,
            summary_message,
            last_seen: self.sequence,
            repeat_count: 0,
        });
        lines.push(line);
        lines
    }

    fn drain_expired(&mut self) -> Vec<DedupLine> {
        let mut lines = Vec::new();
        self.entries.retain(|entry| {
            if self.sequence.saturating_sub(entry.last_seen) <= LOG_DEDUP_WINDOW {
                return true;
            }

            if let Some(repeated) = entry.repeated_line() {
                lines.push(repeated);
            }
            false
        });
        lines
    }
}

impl DedupEntry {
    fn repeated_line(&self) -> Option<DedupLine> {
        if self.repeat_count == 0 {
            return None;
        }

        Some(DedupLine {
            level: self.key.level,
            channel_id: self.key.channel_id,
            listener_id: self.key.listener_id,
            message: format!(
                "{} (repeated {} time{})",
                self.summary_message,
                self.repeat_count,
                if self.repeat_count == 1 { "" } else { "s" }
            ),
        })
    }
}

fn ffmpeg_log_fingerprint(message: &str) -> (String, String) {
    const MATROSKA_PREFIX: &str = "[matroska @ 0x";
    const NEGATIVE_PTS: &str = "] failed to avoid negative pts ";
    const NEW_CLUSTER: &str = "] Starting new cluster due to timestamp";

    if let Some(rest) = message.strip_prefix(MATROSKA_PREFIX)
        && let Some((address, rest)) = rest.split_once(NEGATIVE_PTS)
        && !address.is_empty()
        && address
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        && let Some((pts, stream)) = rest.split_once(" in stream ")
        && pts.parse::<i64>().is_ok()
    {
        let stream = stream.trim_end_matches('.');

        if !stream.is_empty() && stream.chars().all(|character| character.is_ascii_digit()) {
            let summary = format!("[matroska] failed to avoid negative pts in stream {stream}");

            return (summary.clone(), summary);
        }
    }

    if let Some(rest) = message.strip_prefix(MATROSKA_PREFIX)
        && let Some((address, suffix)) = rest.split_once(NEW_CLUSTER)
        && !address.is_empty()
        && address
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        && suffix.is_empty()
    {
        let summary = "[matroska] Starting new cluster due to timestamp".to_string();

        return (summary.clone(), summary);
    }

    (message.to_string(), message.to_string())
}

fn remember_unexpected_rtmp_stream(message: &str) {
    let Some(rest) = message.split_once("Unexpected stream ") else {
        return;
    };
    let Some((actual, expected)) = rest.1.split_once(", expecting ") else {
        return;
    };
    let actual = actual.trim();
    let expected = expected.trim();

    if actual.is_empty() || expected.is_empty() {
        return;
    }

    UNEXPECTED_RTMP_STREAM.with(|stream| {
        *stream.borrow_mut() = Some((actual.to_string(), expected.to_string()));
    });
}

#[cfg(test)]
mod tests {
    use std::{
        thread,
        time::{Duration, Instant},
    };

    #[cfg(target_os = "windows")]
    use std::ptr;

    use ffmpeg_next::{ffi, util::log::Level as FfmpegLevel};

    use super::{
        DedupLine, INGEST_LOG_CONTEXT, IngestLogContext, LogDedup, SrtLogDedup, ffmpeg_ingest_tag,
        ffmpeg_log_fingerprint, level_value, mark_ingest_interrupted, should_skip_ffmpeg_log,
        with_ingest_logs,
    };

    #[test]
    fn ingest_log_context_stays_on_the_listener_thread() {
        with_ingest_logs(Some(3), 42, || {
            assert_eq!(
                INGEST_LOG_CONTEXT.with(std::cell::Cell::get),
                Some(IngestLogContext {
                    channel_id: 3,
                    listener_id: 42
                })
            );

            assert_eq!(
                thread::spawn(|| INGEST_LOG_CONTEXT.with(std::cell::Cell::get))
                    .join()
                    .unwrap(),
                None
            );
        });

        assert_eq!(INGEST_LOG_CONTEXT.with(std::cell::Cell::get), None);
    }

    #[test]
    fn ffmpeg_dedup_keeps_live_listeners_separate() {
        let mut dedup = LogDedup::new();
        let first = IngestLogContext {
            channel_id: 3,
            listener_id: 1,
        };
        let second = IngestLogContext {
            channel_id: 3,
            listener_id: 2,
        };

        assert_eq!(
            dedup
                .push_with_context(16, "Invalid level prefix", Some(first))
                .len(),
            1
        );
        assert_eq!(
            dedup
                .push_with_context(16, "Invalid level prefix", Some(second))
                .len(),
            1
        );
        assert!(
            dedup
                .push_with_context(16, "Invalid level prefix", Some(first))
                .is_empty()
        );
        assert_eq!(
            ffmpeg_ingest_tag(Some(2)),
            " <span class=\"log-gray\">[ingest #2]</span>"
        );
        assert!(ffmpeg_ingest_tag(None).is_empty());
    }

    #[test]
    fn srt_receive_buffer_spam_is_rate_limited_per_socket() {
        let mut dedup = SrtLogDedup::new();
        let now = Instant::now();
        let first = "SRT.qr: @123: No room to store incoming packet seqno 100";
        let repeated = "SRT.qr: @123: No room to store incoming packet seqno 101";
        let other = "SRT.qr: @456: No room to store incoming packet seqno 200";

        assert_eq!(dedup.record(first, now).as_deref(), Some(first));
        assert_eq!(dedup.record(repeated, now + Duration::from_secs(1)), None);
        assert_eq!(
            dedup.record(other, now + Duration::from_secs(1)).as_deref(),
            Some(other)
        );
        assert!(
            dedup
                .record(repeated, now + Duration::from_secs(10))
                .unwrap()
                .contains("1 similar warnings suppressed")
        );
        assert_eq!(
            dedup
                .record("SRT connection failed", now + Duration::from_secs(11))
                .as_deref(),
            Some("SRT connection failed")
        );
    }

    #[test]
    fn deduplicates_consecutive_identical_lines() {
        let mut dedup = LogDedup::new();

        assert_eq!(
            dedup.push(24, "same"),
            vec![DedupLine {
                level: 24,
                channel_id: 0,
                listener_id: None,
                message: "same".to_string(),
            }]
        );
        assert!(dedup.push(24, "same").is_empty());
        assert_eq!(dedup.push(24, "next")[0].message, "next");

        let mut flushed = Vec::new();

        for index in 0..7 {
            flushed.extend(dedup.push(24, &format!("filler {index}")));
        }
        assert!(flushed.iter().any(|line| {
            line.message == "same (repeated 1 time)" && line.level == 24 && line.channel_id == 0
        }));
    }

    #[test]
    fn deduplicates_identical_lines_interleaved_within_six_lines() {
        let mut dedup = LogDedup::new();

        assert_eq!(dedup.push(24, "one")[0].message, "one");
        assert_eq!(dedup.push(24, "two")[0].message, "two");
        assert!(dedup.push(24, "one").is_empty());
        assert!(dedup.push(24, "two").is_empty());

        let mut flushed = Vec::new();

        for index in 0..7 {
            flushed.extend(dedup.push(24, &format!("filler {index}")));
        }
        assert!(
            flushed
                .iter()
                .any(|line| line.message == "one (repeated 1 time)")
        );
        assert!(
            flushed
                .iter()
                .any(|line| line.message == "two (repeated 1 time)")
        );
    }

    #[test]
    fn emits_a_line_again_after_it_leaves_the_six_line_window() {
        let mut dedup = LogDedup::new();

        assert_eq!(dedup.push(24, "same").len(), 1);

        for index in 0..6 {
            dedup.push(24, &format!("different {index}"));
        }
        assert_eq!(dedup.push(24, "same")[0].message, "same");
    }

    #[test]
    fn fingerprints_matroska_timestamp_spam() {
        let mut dedup = LogDedup::new();
        let first =
            "[matroska @ 0x7f2cd59f42c0] failed to avoid negative pts -3326966 in stream 1.";
        let second =
            "[matroska @ 0x7f2cd59f42c0] failed to avoid negative pts -3326946 in stream 1.";

        assert_eq!(dedup.push(24, first)[0].message, first);
        assert!(dedup.push(24, second).is_empty());

        let mut flushed = Vec::new();

        for index in 0..7 {
            flushed.extend(dedup.push(24, &format!("filler {index}")));
        }
        assert!(flushed.iter().any(|line| {
            line.message == "[matroska] failed to avoid negative pts in stream 1 (repeated 1 time)"
        }));

        assert_eq!(
            ffmpeg_log_fingerprint("[matroska @ 0xaaaa] Starting new cluster due to timestamp"),
            (
                "[matroska] Starting new cluster due to timestamp".to_string(),
                "[matroska] Starting new cluster due to timestamp".to_string(),
            )
        );
    }

    #[test]
    fn level_is_part_of_the_dedup_key() {
        let mut dedup = LogDedup::new();

        assert_eq!(dedup.push(24, "same").len(), 1);
        assert_eq!(
            dedup.push(16, "same"),
            vec![DedupLine {
                level: 16,
                channel_id: 0,
                listener_id: None,
                message: "same".to_string(),
            }]
        );
    }

    #[test]
    fn maps_ffmpeg_levels_to_av_log_constants() {
        assert_eq!(level_value(FfmpegLevel::Info), ffi::AV_LOG_INFO);
        assert_eq!(level_value(FfmpegLevel::Warning), ffi::AV_LOG_WARNING);
        assert_eq!(level_value(FfmpegLevel::Error), ffi::AV_LOG_ERROR);
    }

    #[test]
    fn skips_ffmpeg_logs_with_regex_patterns() {
        assert!(should_skip_ffmpeg_log(
            "Opening '/tmp/input.mp4' for reading"
        ));
        assert!(should_skip_ffmpeg_log(
            "Opening 'rtmp://127.0.0.1/live/in' for reading"
        ));
        assert!(should_skip_ffmpeg_log(
            "Opening '/tmp/out.ts.tmp' for writing"
        ));
        assert!(!should_skip_ffmpeg_log("Unexpected stream 1, expecting 0"));
    }

    #[test]
    fn skips_ingest_errors_after_an_intentional_interrupt() {
        with_ingest_logs(Some(1), 7, || {
            mark_ingest_interrupted();
            assert!(should_skip_ffmpeg_log(
                "Cannot open connection tcp://127.0.0.1:1936"
            ));
        });

        assert!(!should_skip_ffmpeg_log(
            "Cannot open connection tcp://127.0.0.1:1936"
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_callback_receives_and_formats_ffmpeg_logs() {
        super::clear_unexpected_rtmp_stream();
        super::set_log_callback();

        unsafe {
            ffi::av_log(
                ptr::null_mut(),
                ffi::AV_LOG_ERROR,
                c"Unexpected stream actual, expecting expected\n".as_ptr(),
            );
        }

        assert_eq!(
            super::take_unexpected_rtmp_stream(),
            Some(("actual".to_string(), "expected".to_string()))
        );
    }
}
