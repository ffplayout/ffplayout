use ffmpeg_next::{ffi, util::log::Level};
use log::{debug, error, info, trace, warn};
use std::{
    cell::RefCell,
    ffi::CStr,
    os::raw::{c_char, c_int, c_void},
};

const DEFAULT_FFMPEG_LOG_LEVEL: Level = Level::Warning;
const FFMPEG_LOG_TARGET: &str = "ffmpeg";
const SKIPPED_FFMPEG_LOG_MESSAGES: &[&str] = &["Could not update timestamps for skipped samples"];

thread_local! {
    static UNEXPECTED_RTMP_STREAM: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

pub(crate) fn init() {
    ffmpeg_next::util::log::set_level(configured_level());
    unsafe {
        ffi::av_log_set_callback(Some(log_callback));
    }
}

pub(crate) fn clear_unexpected_rtmp_stream() {
    UNEXPECTED_RTMP_STREAM.with(|stream| {
        *stream.borrow_mut() = None;
    });
}

pub(crate) fn take_unexpected_rtmp_stream() -> Option<(String, String)> {
    UNEXPECTED_RTMP_STREAM.with(|stream| stream.borrow_mut().take())
}

fn configured_level() -> Level {
    std::env::var("MY_FFMPEG_LOG_LEVEL")
        .ok()
        .as_deref()
        .and_then(parse_level)
        .unwrap_or(DEFAULT_FFMPEG_LOG_LEVEL)
}

fn parse_level(level: &str) -> Option<Level> {
    match level.to_ascii_lowercase().as_str() {
        "quiet" | "off" => Some(Level::Quiet),
        "panic" => Some(Level::Panic),
        "fatal" => Some(Level::Fatal),
        "error" => Some(Level::Error),
        "warn" | "warning" => Some(Level::Warning),
        "info" => Some(Level::Info),
        "verbose" => Some(Level::Verbose),
        "debug" => Some(Level::Debug),
        "trace" => Some(Level::Trace),
        _ => None,
    }
}

unsafe extern "C" fn log_callback(
    avcl: *mut c_void,
    level: c_int,
    fmt: *const c_char,
    vl: *mut ffi::__va_list_tag,
) {
    if level > unsafe { ffi::av_log_get_level() } {
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
