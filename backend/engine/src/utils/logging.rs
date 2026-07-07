use std::{
    cell::RefCell,
    os::raw::c_int,
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicI32, Ordering},
    },
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{
    ffi::CStr,
    os::raw::{c_char, c_void},
};

use ffmpeg_next::{ffi, util::log::Level as FfmpegLevel};
use log::{debug, error, info, trace, warn};
use regex::Regex;

use super::config::LogLevel;

const FFMPEG_LOG_TARGET: &str = "ffmpeg";
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

thread_local! {
    static UNEXPECTED_RTMP_STREAM: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static INGEST_LOG_CONTEXT: RefCell<Option<i32>> = const { RefCell::new(None) };
}

static FFMPEG_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);
static INGEST_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);
static CHANNEL_ID: AtomicI32 = AtomicI32::new(0);
static LOG_DEDUP: Mutex<LogDedup> = Mutex::new(LogDedup::new());
static SKIPPED_FFMPEG_LOG_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
static USER_SKIPPED_FFMPEG_LOG_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(target_os = "linux")]
type FfmpegVaList = *mut ffi::__va_list_tag;
#[cfg(target_os = "macos")]
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
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn set_log_callback() {
    unsafe {
        ffi::av_log_set_callback(Some(log_callback));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn set_log_callback() {}

pub(crate) fn with_ingest_logs<T>(channel_id: Option<i32>, operation: impl FnOnce() -> T) -> T {
    INGEST_LOG_CONTEXT.with(|context| {
        let previous = context.replace(channel_id);
        let result = operation();
        context.replace(previous);
        result
    })
}

pub(crate) fn clear_unexpected_rtmp_stream() {
    UNEXPECTED_RTMP_STREAM.with(|stream| {
        *stream.borrow_mut() = None;
    });
}

pub(crate) fn take_unexpected_rtmp_stream() -> Option<(String, String)> {
    UNEXPECTED_RTMP_STREAM.with(|stream| stream.borrow_mut().take())
}

fn configured_level() -> c_int {
    INGEST_LOG_CONTEXT.with(|context| {
        if context.borrow().is_some() {
            INGEST_LOG_LEVEL.load(Ordering::Relaxed)
        } else {
            FFMPEG_LOG_LEVEL.load(Ordering::Relaxed)
        }
    })
}

fn log_channel_id() -> i32 {
    INGEST_LOG_CONTEXT.with(|context| {
        context
            .borrow()
            .unwrap_or_else(|| CHANNEL_ID.load(Ordering::Relaxed))
    })
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe extern "C" fn log_callback(
    avcl: *mut c_void,
    level: c_int,
    fmt: *const c_char,
    vl: FfmpegVaList,
) {
    if level > unsafe { ffi::av_log_get_level() } || level > configured_level() {
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
        log_line(level, line);
    }
}

fn should_skip_ffmpeg_log(message: &str) -> bool {
    skipped_ffmpeg_log_patterns()
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

fn log_line(level: c_int, message: &str) {
    remember_unexpected_rtmp_stream(message);

    let mut dedup = LOG_DEDUP.lock().unwrap_or_else(PoisonError::into_inner);
    for repeated in dedup.push(level, message) {
        write_log_line(repeated.level, repeated.channel_id, &repeated.message);
    }
}

fn write_log_line(level: c_int, channel_id: i32, message: &str) {
    if level <= ffi::AV_LOG_ERROR {
        error!(target: FFMPEG_LOG_TARGET, channel = channel_id; "[ffmpeg] {message}");
    } else if level <= ffi::AV_LOG_WARNING {
        warn!(target: FFMPEG_LOG_TARGET, channel = channel_id; "[ffmpeg] {message}");
    } else if level <= ffi::AV_LOG_INFO {
        info!(target: FFMPEG_LOG_TARGET, channel = channel_id; "[ffmpeg] {message}");
    } else if level <= ffi::AV_LOG_DEBUG {
        debug!(target: FFMPEG_LOG_TARGET, channel = channel_id; "[ffmpeg] {message}");
    } else {
        trace!(target: FFMPEG_LOG_TARGET, channel = channel_id; "[ffmpeg] {message}");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DedupLine {
    level: c_int,
    channel_id: i32,
    message: String,
}

#[derive(Debug)]
struct LogDedup {
    last: Option<DedupLine>,
    repeat_count: usize,
}

impl LogDedup {
    const fn new() -> Self {
        Self {
            last: None,
            repeat_count: 0,
        }
    }

    fn push(&mut self, level: c_int, message: &str) -> Vec<DedupLine> {
        let line = DedupLine {
            level,
            channel_id: log_channel_id(),
            message: message.to_string(),
        };

        if self.last.as_ref() == Some(&line) {
            self.repeat_count += 1;
            if self.repeat_count >= LOG_DEDUP_FLUSH_THRESHOLD {
                let repeated = self.repeated_line();
                self.repeat_count = 0;
                return repeated.into_iter().collect();
            }
            return Vec::new();
        }

        let mut lines = Vec::new();
        lines.extend(self.repeated_line());
        lines.push(line.clone());
        self.last = Some(line);
        self.repeat_count = 0;
        lines
    }

    fn repeated_line(&self) -> Option<DedupLine> {
        let last = self.last.as_ref()?;
        if self.repeat_count == 0 {
            return None;
        }

        Some(DedupLine {
            level: last.level,
            channel_id: last.channel_id,
            message: format!(
                "{} (repeated {} time{})",
                last.message,
                self.repeat_count,
                if self.repeat_count == 1 { "" } else { "s" }
            ),
        })
    }
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
    use ffmpeg_next::{ffi, util::log::Level as FfmpegLevel};

    use super::{DedupLine, LogDedup, level_value, should_skip_ffmpeg_log};

    #[test]
    fn deduplicates_consecutive_identical_lines() {
        let mut dedup = LogDedup::new();

        assert_eq!(
            dedup.push(24, "same"),
            vec![DedupLine {
                level: 24,
                channel_id: 0,
                message: "same".to_string(),
            }]
        );
        assert!(dedup.push(24, "same").is_empty());
        assert_eq!(
            dedup.push(24, "next"),
            vec![
                DedupLine {
                    level: 24,
                    channel_id: 0,
                    message: "same (repeated 1 time)".to_string(),
                },
                DedupLine {
                    level: 24,
                    channel_id: 0,
                    message: "next".to_string(),
                },
            ]
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
}
