use std::{
    cell::RefCell,
    ffi::CStr,
    os::raw::{c_char, c_int, c_void},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicI32, Ordering},
    },
};

use ffmpeg_next::{ffi, util::log::Level as FfmpegLevel};
use log::{debug, error, info, trace, warn};

const FFMPEG_LOG_TARGET: &str = "ffmpeg";
const SKIPPED_FFMPEG_LOG_MESSAGES: &[&str] = &[
    "Could not update timestamps for skipped samples",
    "for writing",
    "for reading",
];
const LOG_DEDUP_FLUSH_THRESHOLD: usize = 100;

use super::config::LogLevel;

thread_local! {
    static UNEXPECTED_RTMP_STREAM: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static INGEST_LOG_CONTEXT: RefCell<Option<i32>> = const { RefCell::new(None) };
}

static FFMPEG_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);
static INGEST_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);
static CHANNEL_ID: AtomicI32 = AtomicI32::new(0);
static LOG_DEDUP: Mutex<LogDedup> = Mutex::new(LogDedup::new());

pub(crate) fn init(ffmpeg_level: LogLevel, ingest_level: LogLevel, channel_id: Option<i32>) {
    let ffmpeg_level = ffmpeg_level.as_ffmpeg_level();
    let ingest_level = ingest_level.as_ffmpeg_level();
    FFMPEG_LOG_LEVEL.store(level_value(ffmpeg_level), Ordering::Relaxed);
    INGEST_LOG_LEVEL.store(level_value(ingest_level), Ordering::Relaxed);
    CHANNEL_ID.store(channel_id.unwrap_or(0), Ordering::Relaxed);
    ffmpeg_next::util::log::set_level(max_level(ffmpeg_level, ingest_level));
    unsafe {
        ffi::av_log_set_callback(Some(log_callback));
    }
}

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

unsafe extern "C" fn log_callback(
    avcl: *mut c_void,
    level: c_int,
    fmt: *const c_char,
    vl: *mut ffi::__va_list_tag,
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
    SKIPPED_FFMPEG_LOG_MESSAGES
        .iter()
        .any(|skipped| message.contains(skipped))
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

    use super::{DedupLine, LogDedup, level_value};

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
}
