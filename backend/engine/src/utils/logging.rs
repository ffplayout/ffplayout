use ffmpeg_next::{ffi, util::log::Level as FfmpegLevel};
use log::{debug, error, info, trace, warn};
use std::{
    cell::RefCell,
    ffi::CStr,
    os::raw::{c_char, c_int, c_void},
    sync::atomic::{AtomicI32, Ordering},
};

const FFMPEG_LOG_TARGET: &str = "ffmpeg";
const SKIPPED_FFMPEG_LOG_MESSAGES: &[&str] = &["Could not update timestamps for skipped samples"];

use super::config::LogLevel;

thread_local! {
    static UNEXPECTED_RTMP_STREAM: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static INGEST_LOG_CONTEXT: RefCell<bool> = const { RefCell::new(false) };
}

static FFMPEG_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);
static INGEST_LOG_LEVEL: AtomicI32 = AtomicI32::new(ffi::AV_LOG_WARNING);

pub(crate) fn init(ffmpeg_level: LogLevel, ingest_level: LogLevel) {
    let ffmpeg_level = ffmpeg_level.as_ffmpeg_level();
    let ingest_level = ingest_level.as_ffmpeg_level();
    FFMPEG_LOG_LEVEL.store(level_value(ffmpeg_level), Ordering::Relaxed);
    INGEST_LOG_LEVEL.store(level_value(ingest_level), Ordering::Relaxed);
    ffmpeg_next::util::log::set_level(max_level(ffmpeg_level, ingest_level));
    unsafe {
        ffi::av_log_set_callback(Some(log_callback));
    }
}

pub(crate) fn with_ingest_logs<T>(operation: impl FnOnce() -> T) -> T {
    INGEST_LOG_CONTEXT.with(|context| {
        let previous = context.replace(true);
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
        if *context.borrow() {
            INGEST_LOG_LEVEL.load(Ordering::Relaxed)
        } else {
            FFMPEG_LOG_LEVEL.load(Ordering::Relaxed)
        }
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
    level as c_int
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

    if level <= ffi::AV_LOG_ERROR {
        error!(target: FFMPEG_LOG_TARGET, "[ffmpeg] {message}");
    } else if level <= ffi::AV_LOG_WARNING {
        warn!(target: FFMPEG_LOG_TARGET, "[ffmpeg] {message}");
    } else if level <= ffi::AV_LOG_INFO {
        info!(target: FFMPEG_LOG_TARGET, "[ffmpeg] {message}");
    } else if level <= ffi::AV_LOG_DEBUG {
        debug!(target: FFMPEG_LOG_TARGET, "[ffmpeg] {message}");
    } else {
        trace!(target: FFMPEG_LOG_TARGET, "[ffmpeg] {message}");
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
