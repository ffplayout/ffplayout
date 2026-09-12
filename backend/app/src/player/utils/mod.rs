use std::{
    ffi::OsStr,
    fmt,
    io::Error,
    path::{Path, PathBuf},
    process::exit,
    str::FromStr,
};

use chrono::{TimeDelta, prelude::*};
use chrono_tz::Tz;
use log::*;
use regex::Regex;
use reqwest::header;
use serde::{Deserialize, Serialize, de::Deserializer};
use serde_json::{Map, Value, json};
use tokio::{
    fs::{File, metadata},
    io::{AsyncReadExt, AsyncWriteExt},
};

pub mod import;
pub mod json_serializer;
pub mod json_validate;

use crate::{
    player::controller::{
        ChannelManager,
        ProcessUnit::{self, *},
    },
    utils::{
        config::{OutputMode::*, PlayoutConfig},
        errors::ProcessError,
        time_machine::time_now,
    },
};
pub use json_serializer::{JsonPlaylist, read_json};

pub type MediaProbe = ff_engine::EngineMediaProbe;
pub type SilenceDetection = ff_engine::SilenceDetection;

/// Still images have no inherent media duration. Keep this aligned with the
/// duration assigned by the playlist editor when an image is dropped into it.
pub const DEFAULT_IMAGE_DURATION: f64 = 10.0;
/// Fallback playlist slot for a live transport without an explicit `out`.
/// A bounded slot lets the schedule advance and reconnect to a source that
/// ends unexpectedly; operators can always choose a longer explicit `out`.
pub const DEFAULT_LIVE_SOURCE_DURATION: f64 = 60.0;

pub(crate) fn is_live_source(source: &str) -> bool {
    ff_engine::is_live_input(source)
}

/// Live transports have no stable source timeline. Playlist `in` is therefore
/// ignored and an omitted `out` becomes a finite scheduling slot. Since a
/// live transport has no meaningful file duration, the slot is also its
/// effective duration; this prevents a playlist `out` beyond that duration.
pub(crate) fn normalize_live_source_timing(media: &mut Media) {
    if !is_live_source(&media.source) {
        return;
    }

    media.seek = 0.0;
    if !media.out.is_finite() || media.out <= 0.0 {
        media.out = DEFAULT_LIVE_SOURCE_DURATION;
    }
    if !media.duration.is_finite() || media.duration < media.out {
        media.duration = media.out;
    }
}

pub(crate) fn is_image_source(source: &str) -> bool {
    let source = source.split('?').next().unwrap_or(source);
    let Some(extension) = Path::new(source).extension().and_then(OsStr::to_str) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "apng"
            | "avif"
            | "bmp"
            | "exr"
            | "gif"
            | "jpeg"
            | "jpg"
            | "png"
            | "psd"
            | "tga"
            | "tif"
            | "tiff"
            | "webp"
    )
}

pub async fn probe_media(input: impl AsRef<std::path::Path>) -> Result<MediaProbe, ProcessError> {
    let path = input.as_ref().to_string_lossy().to_string();
    ff_engine::probe_media(&path).map_err(|error| ProcessError::Ffprobe(error.to_string()))
}

pub async fn detect_audio_silence(
    input: impl AsRef<std::path::Path>,
    seek_seconds: f64,
    duration_seconds: f64,
) -> Result<SilenceDetection, ProcessError> {
    let path = input.as_ref().to_string_lossy().to_string();
    let window_seconds = 30.0_f64;
    let duration_seconds = duration_seconds.max(0.0);
    if duration_seconds < window_seconds {
        return Ok(SilenceDetection {
            silent: false,
            analyzed_seconds: duration_seconds,
        });
    }
    let analysis_seconds = window_seconds;
    let analysis_seek_seconds = seek_seconds.max(0.0) + (duration_seconds - analysis_seconds) / 2.0;
    tokio::task::spawn_blocking(move || {
        ff_engine::detect_audio_silence(
            &path,
            analysis_seek_seconds,
            analysis_seconds,
            -30.0,
            analysis_seconds,
        )
        .map_err(|error| ProcessError::Ffprobe(error.to_string()))
    })
    .await
    .map_err(|error| ProcessError::Custom(error.to_string()))?
}

/// map media struct to json object
pub fn get_media_map(media: Media) -> Value {
    let mut obj = json!({
        "in": media.seek,
        "out": media.out,
        "duration": media.duration,
        "ad": media.ad,
        "source": media.source,
    });

    if let Some(title) = media.title {
        obj.as_object_mut()
            .unwrap()
            .insert("title".to_string(), Value::String(title));
    }

    obj
}

/// prepare json object for response
pub async fn get_data_map(manager: &ChannelManager) -> Map<String, Value> {
    let media = manager
        .current_media
        .lock()
        .await
        .clone()
        .unwrap_or_else(Media::default);
    let channel = manager.channel.lock().await.clone();
    let config = manager.config.read().await.processing.clone();
    let ingest_is_alive = manager.playback_control.lock().await.live_active();

    let mut data_map = Map::new();
    let current_time = time_in_seconds(&channel.timezone);
    let shift = channel.time_shift;
    let begin = media.begin.unwrap_or(0.0) - shift;
    let played_time = current_time - begin;

    data_map.insert("index".to_string(), json!(media.index));
    data_map.insert("ingest".to_string(), json!(ingest_is_alive));
    data_map.insert("mode".to_string(), json!(config.mode));
    data_map.insert(
        "shift".to_string(),
        json!((shift * 1000.0).round() / 1000.0),
    );
    data_map.insert(
        "elapsed".to_string(),
        json!((played_time * 1000.0).round() / 1000.0),
    );
    data_map.insert("media".to_string(), get_media_map(media));
    if let Ok(audio_level) = manager.audio_level.lock()
        && let Some(audio_level) = *audio_level
    {
        data_map.insert(
            "audio".to_string(),
            json!({
                "rms_db": (f64::from(audio_level.rms_db) * 10.0).round() / 10.0,
                "peak_db": (f64::from(audio_level.peak_db) * 10.0).round() / 10.0,
            }),
        );
    }
    if let Some(metrics) = manager.loudness_meter.metrics() {
        let round = |value: Option<f64>| value.map(|value| (value * 10.0).round() / 10.0);
        data_map.insert(
            "loudness".to_string(),
            json!({
                "momentary_lufs": round(metrics.momentary_lufs),
                "short_term_lufs": round(metrics.short_term_lufs),
                "integrated_lufs": round(metrics.integrated_lufs),
                "true_peak_dbtp": round(metrics.true_peak_dbtp),
            }),
        );
    }

    data_map
}

/// Video clip struct to hold some important states and comments for current media.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Media {
    #[serde(skip_serializing, skip_deserializing)]
    pub begin: Option<f64>,

    #[serde(skip_serializing, skip_deserializing)]
    pub index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "in")]
    pub seek: f64,
    pub out: f64,
    pub duration: f64,

    #[serde(skip_serializing, skip_deserializing)]
    pub duration_audio: f64,

    #[serde(
        default,
        alias = "category",
        deserialize_with = "deserialize_ad",
        skip_serializing_if = "is_false"
    )]
    pub ad: bool,
    #[serde(deserialize_with = "null_string")]
    pub source: String,

    #[serde(
        default,
        deserialize_with = "null_string",
        skip_serializing_if = "is_empty_string"
    )]
    pub audio: String,

    #[serde(default, skip_serializing_if = "is_empty_string")]
    pub custom_filter: String,

    #[serde(skip_serializing, skip_deserializing)]
    pub probe: Option<MediaProbe>,

    #[serde(skip_serializing, skip_deserializing)]
    pub probe_audio: Option<MediaProbe>,

    #[serde(skip_serializing, skip_deserializing)]
    pub last_ad: bool,

    #[serde(skip_serializing, skip_deserializing)]
    pub next_ad: bool,

    #[serde(default, skip_serializing, skip_deserializing)]
    pub skip: bool,

    #[serde(default, skip_serializing, skip_deserializing)]
    pub is_placeholder: bool,

    #[serde(default, skip_serializing)]
    pub unit: ProcessUnit,
}

impl Media {
    pub async fn new(index: usize, src: &str, do_probe: bool) -> Self {
        let source_is_live = is_live_source(src);
        let mut duration = if is_image_source(src) {
            DEFAULT_IMAGE_DURATION
        } else {
            Default::default()
        };
        let mut probe = None;

        if do_probe
            && !source_is_live
            && (is_remote(src) || Path::new(src).is_file())
            && let Ok(p) = probe_media(src).await
        {
            probe = Some(p.clone());

            duration = p.format.duration.unwrap_or_default();
        }

        if source_is_live {
            duration = DEFAULT_LIVE_SOURCE_DURATION;
        }

        Self {
            begin: None,
            index: Some(index),
            title: None,
            seek: 0.0,
            out: duration,
            duration,
            duration_audio: 0.0,
            ad: false,
            source: src.to_string(),
            audio: String::new(),
            custom_filter: String::new(),
            probe,
            probe_audio: None,
            last_ad: false,
            next_ad: false,
            skip: false,
            is_placeholder: false,
            unit: Decoder,
        }
    }

    pub async fn add_probe(&mut self, check_audio: bool) -> Result<(), String> {
        let mut errors = vec![];
        let source_is_live = is_live_source(&self.source);

        if self.duration <= 0.0 && is_image_source(&self.source) {
            // A scheduled image may have an explicit `out` duration in a
            // playlist. Images are looped by the engine, so treat that as its
            // usable duration; otherwise use the same default as the editor.
            self.duration = self.out.max(DEFAULT_IMAGE_DURATION);
            if self.out <= 0.0 {
                self.out = self.duration;
            }
        }

        if !source_is_live && self.probe.is_none() {
            match probe_media(&self.source).await {
                Ok(probe) => {
                    self.probe = Some(probe.clone());

                    if let Some(dur) = probe
                        .format
                        .duration
                        .filter(|d| !is_close(*d, self.duration, 0.5))
                    {
                        self.duration = dur;

                        if self.out == 0.0 {
                            self.out = dur;
                        }
                    }
                }
                Err(e) => errors.push(e.to_string()),
            };
        }

        if check_audio
            && !self.audio.is_empty()
            && self.probe_audio.is_none()
            && !is_live_source(&self.audio)
            && (is_remote(&self.audio) || Path::new(&self.audio).is_file())
        {
            match probe_media(&self.audio).await {
                Ok(probe) => {
                    self.probe_audio = Some(probe.clone());

                    if !probe.audio.is_empty() {
                        self.duration_audio = probe.audio[0].duration.unwrap_or_default();
                    }
                }
                Err(e) => errors.push(e.to_string()),
            }
        }

        if !errors.is_empty() {
            return Err(errors.join(", "));
        }

        Ok(())
    }
}

impl Default for Media {
    fn default() -> Self {
        Self {
            begin: None,
            index: Some(0),
            title: None,
            seek: 0.0,
            out: 0.0,
            duration: 0.0,
            duration_audio: 0.0,
            ad: false,
            source: String::new(),
            audio: String::new(),
            custom_filter: String::new(),
            probe: None,
            probe_audio: None,
            last_ad: false,
            next_ad: false,
            skip: false,
            is_placeholder: false,
            unit: Decoder,
        }
    }
}

impl PartialEq for Media {
    fn eq(&self, other: &Self) -> bool {
        self.title == other.title
            && self.seek == other.seek
            && self.out == other.out
            && self.duration == other.duration
            && self.source == other.source
            && self.ad == other.ad
            && self.audio == other.audio
            && self.custom_filter == other.custom_filter
    }
}

impl Eq for Media {}

fn null_string<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Deserialize::deserialize(d).map(|x: Option<_>| x.unwrap_or_default())
}

fn deserialize_ad<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Bool(ad) => Ok(ad),
        Value::String(category) => Ok(category == "advertisement"),
        Value::Null => Ok(false),
        value => Err(serde::de::Error::custom(format!(
            "expected an ad boolean or legacy category string, got {value}"
        ))),
    }
}

const fn is_false(value: &bool) -> bool {
    !*value
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_empty_string(st: &String) -> bool {
    *st == String::new()
}

pub async fn json_reader(path: &PathBuf) -> Result<JsonPlaylist, Error> {
    let mut f = File::options().read(true).write(false).open(path).await?;
    let mut contents = String::new();
    f.read_to_string(&mut contents).await?;
    let p = serde_json::from_str(&contents)?;

    Ok(p)
}

pub async fn json_writer(path: &PathBuf, data: JsonPlaylist) -> Result<(), Error> {
    let mut f = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(path)
        .await?;
    let contents = serde_json::to_string_pretty(&data)?;
    f.write_all(contents.as_bytes()).await?;

    Ok(())
}

/// Get current time in seconds.
pub fn time_in_seconds(timezone: &Option<Tz>) -> f64 {
    let local: DateTime<Tz> = time_now(timezone);

    (local.hour() * 3600 + local.minute() * 60 + local.second()) as f64
        + (local.nanosecond() as f64 / 1000000000.0)
}

/// Get current date for playlist, but check time with conditions:
///
/// - When time is before playlist start, get date from yesterday.
/// - When given next_start is over target length (normally a full day), get date from tomorrow.
pub fn get_date(seek: bool, start: f64, get_next: bool, timezone: &Option<Tz>) -> String {
    let local: DateTime<Tz> = time_now(timezone);

    if seek && start > time_in_seconds(timezone) {
        return (local - TimeDelta::try_days(1).unwrap())
            .format("%Y-%m-%d")
            .to_string();
    }

    if start == 0.0 && get_next && time_in_seconds(timezone) > 86397.9 {
        return (local + TimeDelta::try_days(1).unwrap())
            .format("%Y-%m-%d")
            .to_string();
    }

    local.format("%Y-%m-%d").to_string()
}

pub fn time_from_header(headers: &header::HeaderMap) -> Option<DateTime<Local>> {
    if let Some(time) = headers.get(header::LAST_MODIFIED)
        && let Ok(t) = time.to_str()
    {
        let time = DateTime::parse_from_rfc2822(t);
        let date_time: DateTime<Local> = time.unwrap().into();
        return Some(date_time);
    };

    None
}

/// Get file modification time.
pub async fn modified_time(path: &str) -> Option<String> {
    if is_remote(path) {
        let response = reqwest::Client::new().head(path).send().await;

        if let Ok(resp) = response
            && resp.status().is_success()
            && let Some(time) = time_from_header(resp.headers())
        {
            return Some(time.to_string());
        }

        return None;
    }

    if let Ok(time) = metadata(path)
        .await
        .and_then(|metadata| metadata.modified())
    {
        let date_time: DateTime<Local> = time.into();
        return Some(date_time.to_string());
    }

    None
}

/// Convert a formatted time string to seconds.
pub fn time_to_sec(time_str: &str, timezone: &Option<Tz>) -> f64 {
    if matches!(time_str, "now" | "" | "none") || !time_str.contains(':') {
        return time_in_seconds(timezone);
    }

    let mut t = time_str.split(':').filter_map(|n| f64::from_str(n).ok());

    t.next().unwrap_or(0.0) * 3600.0 + t.next().unwrap_or(0.0) * 60.0 + t.next().unwrap_or(0.0)
}

/// Convert floating number (seconds) to a formatted time string.
pub fn sec_to_time(sec: f64) -> String {
    let s = (sec * 1000.0).round() / 1000.0;

    format!(
        "{:0>2}:{:0>2}:{:06.3}",
        (s / 3600.0) as i32,
        (s / 60.0 % 60.0) as i32,
        (s % 60.0),
    )
}

/// get file extension
pub fn file_extension(filename: &Path) -> Option<&str> {
    filename.extension().and_then(OsStr::to_str)
}

/// Test if given numbers are close to each other,
/// with a third number for setting the maximum range.
pub fn is_close(a: f64, b: f64, to: f64) -> bool {
    (a - b).abs() < to
}

/// add duration from all media clips
pub fn sum_durations(clip_list: &[Media]) -> f64 {
    clip_list.iter().map(|item| item.out).sum()
}

/// Get delta between clip start and current time. This value we need to check,
/// if we still in sync.
///
/// We also get here the global delta between clip start and time when a new playlist should start.
pub fn get_delta(config: &PlayoutConfig, begin: &f64) -> (f64, f64) {
    let mut current_time = time_in_seconds(&config.channel.timezone);
    let start = config.playlist.start_sec.unwrap();
    let length = config.playlist.length_sec.unwrap_or(86400.0);
    let mut target_length = 86400.0;

    if length > 0.0 && length != target_length {
        target_length = length;
    }

    if begin == &start && start == 0.0 && 86400.0 - current_time < 4.0 {
        current_time -= 86400.0;
    } else if start >= current_time && begin != &start {
        current_time += 86400.0;
    }

    let mut current_delta = begin - current_time;

    if is_close(
        current_delta.abs(),
        86400.0,
        config.general.stop_threshold + 2.0,
    ) {
        current_delta = current_delta.abs() - 86400.0;
    }

    let total_delta = if current_time < start {
        start - current_time
    } else {
        target_length + start - current_time
    };

    (current_delta, total_delta)
}

pub fn is_remote(path: &str) -> bool {
    Regex::new(r"^(https?|rtmps?|rts?p|udp|tcp|srt|rist)://.*")
        .unwrap()
        .is_match(&path.to_lowercase())
}

/// Check if file can include or has to exclude.
/// For example when a file is on given HLS output path, it should exclude.
/// Or when the file extension is set under storage config it can be include.
pub fn include_file_extension(config: &PlayoutConfig, file_path: &Path) -> bool {
    let mut include = false;

    if let Some(ext) = file_extension(file_path)
        && config.storage.extensions.contains(&ext.to_lowercase())
    {
        include = true;
    }

    if config.output.mode == HLS {
        let playlist_path = config
            .channel
            .public
            .join("live")
            .join(format!("{}.m3u8", config.output.hls_playlist_name));
        if playlist_path
            .parent()
            .is_some_and(|parent| file_path.starts_with(parent))
        {
            include = false;
        }
    }

    include
}

/// Generate a vector with dates, from given range.
pub fn get_date_range(id: i32, date_range: &[String]) -> Vec<String> {
    let mut range = vec![];

    let start = match NaiveDate::parse_from_str(&date_range[0], "%Y-%m-%d") {
        Ok(s) => s,
        Err(_) => {
            error!(channel = id; "date format error in: <span class=\"log-number\">{:?}</span>", date_range[0]);
            exit(1);
        }
    };

    let end = match NaiveDate::parse_from_str(&date_range[2], "%Y-%m-%d") {
        Ok(e) => e,
        Err(_) => {
            error!(channel = id; "date format error in: <span class=\"log-number\">{:?}</span>", date_range[2]);
            exit(1);
        }
    };

    let duration = end.signed_duration_since(start);
    let days = duration.num_days() + 1;

    for day in 0..days {
        range.push(
            (start + TimeDelta::try_days(day).unwrap())
                .format("%Y-%m-%d")
                .to_string(),
        );
    }

    range
}

pub fn custom_format<T: fmt::Display>(template: &str, args: &[T]) -> String {
    let mut filled_template = String::new();
    let mut arg_iter = args.iter().map(T::to_string);
    let mut template_iter = template.chars().peekable();

    while let Some(c) = template_iter.next() {
        if c == '{' {
            match template_iter.peek().copied() {
                Some('{') => {
                    template_iter.next();
                    filled_template.push('{');
                }
                Some('}') => {
                    template_iter.next();

                    if let Some(arg) = arg_iter.next() {
                        filled_template.push_str(&arg);
                    } else {
                        filled_template.push('{');
                        filled_template.push('}');
                    }
                }
                Some(nc) if nc.is_ascii_digit() => {
                    let mut index = String::new();

                    while let Some(nc) = template_iter.peek().copied() {
                        if nc.is_ascii_digit() {
                            index.push(nc);
                            template_iter.next();
                        } else {
                            break;
                        }
                    }

                    if matches!(template_iter.peek(), Some('}')) {
                        template_iter.next();

                        if let Ok(n) = index.parse::<usize>() {
                            if let Some(arg) = args.get(n) {
                                filled_template.push_str(&arg.to_string());
                            } else {
                                filled_template.push('{');
                                filled_template.push_str(&index);
                                filled_template.push('}');
                            }
                        } else {
                            filled_template.push('{');
                            filled_template.push_str(&index);
                            filled_template.push('}');
                        }
                    } else {
                        filled_template.push('{');
                        filled_template.push_str(&index);
                    }
                }
                _ => filled_template.push('{'),
            }
        } else if c == '}' {
            if matches!(template_iter.peek(), Some('}')) {
                template_iter.next();
                filled_template.push('}');
            } else {
                filled_template.push('}');
            }
        } else {
            filled_template.push(c);
        }
    }

    filled_template
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        DEFAULT_IMAGE_DURATION, DEFAULT_LIVE_SOURCE_DURATION, Media, custom_format, is_remote,
        normalize_live_source_timing, probe_media,
    };

    #[tokio::test]
    async fn images_receive_a_default_playout_duration() {
        let media = Media::new(0, "still.jpg", false).await;

        assert_eq!(media.duration, DEFAULT_IMAGE_DURATION);
        assert_eq!(media.out, DEFAULT_IMAGE_DURATION);
    }

    #[tokio::test]
    async fn live_sources_receive_a_bounded_default_without_probing() {
        let media = Media::new(0, "srt://example.invalid:9000", true).await;

        assert_eq!(media.duration, DEFAULT_LIVE_SOURCE_DURATION);
        assert_eq!(media.out, DEFAULT_LIVE_SOURCE_DURATION);
        assert!(media.probe.is_none());
    }

    #[test]
    fn live_sources_ignore_playlist_seek_and_keep_explicit_out() {
        let mut media = Media {
            source: "udp://239.0.0.1:1234".to_string(),
            seek: 42.0,
            out: 120.0,
            ..Media::default()
        };

        normalize_live_source_timing(&mut media);

        assert_eq!(media.seek, 0.0);
        assert_eq!(media.out, 120.0);
        assert_eq!(media.duration, 120.0);
    }

    #[test]
    fn rist_sources_are_treated_as_remote_urls() {
        assert!(is_remote("rist://example.invalid:8193"));
    }

    #[tokio::test]
    async fn add_probe_checks_external_audio_when_main_probe_is_cached() {
        let audio_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/assets/storage/media_mix/audio.mp3");
        let mut media = Media::new(0, "cached-main.mp4", false).await;
        media.probe = Some(probe_media(&audio_path).await.unwrap());
        media.audio = audio_path.to_string_lossy().into_owned();

        media.add_probe(true).await.unwrap();

        assert!(media.probe_audio.is_some());
        assert!(media.duration_audio > 0.0);
    }

    #[test]
    fn legacy_advertisement_category_is_serialized_as_ad() {
        let media: Media = serde_json::from_value(json!({
            "in": 0.0,
            "out": 10.0,
            "duration": 10.0,
            "category": "advertisement",
            "source": "spot.mp4"
        }))
        .unwrap();

        assert!(media.ad);

        let serialized = serde_json::to_value(media).unwrap();
        assert_eq!(serialized["ad"], true);
        assert!(serialized.get("category").is_none());
    }

    #[test]
    fn non_advertisement_legacy_category_is_not_an_ad() {
        let media: Media = serde_json::from_value(json!({
            "in": 0.0,
            "out": 10.0,
            "duration": 10.0,
            "category": "program",
            "source": "program.mp4"
        }))
        .unwrap();

        assert!(!media.ad);

        let serialized = serde_json::to_value(media).unwrap();
        assert!(serialized.get("ad").is_none());
        assert!(serialized.get("category").is_none());
    }

    #[test]
    fn custom_format_keeps_escaped_braces_without_args() {
        assert_eq!(custom_format::<&str>("{{}}", &[]), "{}");
    }

    #[test]
    fn custom_format_replaces_automatic_placeholders() {
        assert_eq!(custom_format("{} {}", &["foo", "bar"]), "foo bar");
    }

    #[test]
    fn custom_format_replaces_indexed_placeholders() {
        assert_eq!(custom_format("{1}-{0}", &["left", "right"]), "right-left");
        assert_eq!(
            custom_format("{10}", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            "10"
        );
    }

    #[test]
    fn custom_format_keeps_out_of_range_placeholders() {
        assert_eq!(custom_format("{9}", &["foo"]), "{9}");
        assert_eq!(custom_format("{}", &[] as &[&str]), "{}");
    }

    #[test]
    fn custom_format_keeps_invalid_or_unclosed_placeholders() {
        assert_eq!(custom_format("{x}", &["foo"]), "{x}");
        assert_eq!(custom_format("{0", &["foo"]), "{0");
        assert_eq!(custom_format("}", &["foo"]), "}");
    }
}
