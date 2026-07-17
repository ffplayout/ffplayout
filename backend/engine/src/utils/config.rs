use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{Arc, PoisonError, RwLock},
};

use ffmpeg_next::{Rational, util::log::Level as FfmpegLevel};

use crate::{AudioEffectsControl, AudioLevelCallback};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsVariant {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub video_bitrate: u64,
    pub audio_bitrate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsSubtitle {
    pub name: String,
    pub language: String,
    pub default: bool,
}

impl HlsSubtitle {
    pub fn validate(&self) -> Result<(), String> {
        validate_stream_map_value("subtitle name", &self.name)?;
        validate_stream_map_value("subtitle language", &self.language)
    }
}

fn validate_stream_map_value(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.chars().any(|ch| ch.is_whitespace() || ch == ',') {
        return Err(format!("{label} must not contain whitespace or ','"));
    }
    Ok(())
}

impl FromStr for HlsVariant {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split(':');
        let name = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or_else(|| "missing variant name".to_string())?;
        if !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(
                "variant name may only contain ASCII letters, numbers, '_' and '-'".to_string(),
            );
        }
        let resolution = parts
            .next()
            .ok_or_else(|| "missing variant resolution".to_string())?;
        let video_bitrate = parts
            .next()
            .ok_or_else(|| "missing variant video bitrate".to_string())?;
        let audio_bitrate = parts.next().unwrap_or("128k");

        if parts.next().is_some() {
            return Err("expected NAME:WIDTHxHEIGHT:VIDEO_BITRATE[:AUDIO_BITRATE]".to_string());
        }

        let (width, height) = resolution
            .split_once('x')
            .ok_or_else(|| "resolution must use WIDTHxHEIGHT".to_string())?;
        let width = width
            .parse::<u32>()
            .map_err(|_| "width must be a positive integer".to_string())?;
        let height = height
            .parse::<u32>()
            .map_err(|_| "height must be a positive integer".to_string())?;
        if width == 0 || height == 0 {
            return Err("width and height must be greater than zero".to_string());
        }

        Ok(Self {
            name: name.to_string(),
            width,
            height,
            video_bitrate: parse_bitrate(video_bitrate)?,
            audio_bitrate: parse_bitrate(audio_bitrate)?,
        })
    }
}

fn parse_bitrate(value: &str) -> Result<u64, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("bitrate must not be empty".to_string());
    }

    let (number, multiplier) = match value.as_bytes().last().copied() {
        Some(b'k') | Some(b'K') => (&value[..value.len() - 1], 1_000),
        Some(b'm') | Some(b'M') => (&value[..value.len() - 1], 1_000_000),
        _ => (value, 1),
    };
    let number = number
        .parse::<u64>()
        .map_err(|_| format!("invalid bitrate {value:?}"))?;
    if number == 0 {
        return Err("bitrate must be greater than zero".to_string());
    }
    Ok(number * multiplier)
}

#[cfg(test)]
mod hls_subtitle_tests {
    use super::HlsSubtitle;

    #[test]
    fn accepts_stream_map_safe_metadata() {
        assert!(
            HlsSubtitle {
                name: "Deutsch".to_string(),
                language: "de-DE".to_string(),
                default: false,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn rejects_values_that_break_stream_map() {
        for name in ["", "Deutsch SD", "Deutsch,SD"] {
            assert!(
                HlsSubtitle {
                    name: name.to_string(),
                    language: "de-DE".to_string(),
                    default: false,
                }
                .validate()
                .is_err()
            );
        }
    }
}

#[cfg(test)]
mod log_level_tests {
    use super::LogLevel;

    #[test]
    fn parses_ui_log_levels() {
        assert_eq!("INFO".parse::<LogLevel>(), Ok(LogLevel::Info));
        assert_eq!("WARNING".parse::<LogLevel>(), Ok(LogLevel::Warning));
        assert_eq!("ERROR".parse::<LogLevel>(), Ok(LogLevel::Error));
    }

    #[test]
    fn rejects_unknown_log_levels() {
        assert!("everything".parse::<LogLevel>().is_err());
    }
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub width: u32,
    pub height: u32,
    pub desktop_window_size: Option<(u32, u32)>,
    pub desktop_fullscreen: bool,
    pub fps: u32,
    pub sample_rate: u32,
    pub video_time_base: Rational,
    pub audio_time_base: Rational,
    pub audio_effects: AudioEffectsControl,
    pub audio_level_callback: Option<AudioLevelCallback>,
    pub logo: Option<LogoConfig>,
    pub text: Option<TextConfig>,
    pub text_overlay_state: TextOverlayState,
    pub stream_type: StreamType,
    pub stream_format: String,
    pub video_codec: String,
    pub video_options: VideoOptions,
    pub audio_codec: String,
    pub audio_bitrate: u64,
    pub ffmpeg_log_level: LogLevel,
    pub ingest_log_level: LogLevel,
    pub ffmpeg_ignore_lines: Vec<String>,
    pub channel_id: Option<i32>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    #[default]
    Rtmp,
    Srt,
    Udp,
    Custom,
}

impl StreamType {
    pub fn muxer(self, custom_format: &str) -> &str {
        match self {
            Self::Rtmp => "flv",
            Self::Srt | Self::Udp => "mpegts",
            Self::Custom => custom_format,
        }
    }
}

pub type VideoOptions = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoOptionKind {
    Select,
    Number,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoOptionChoice {
    pub value: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoOptionVisibility {
    pub key: &'static str,
    pub value: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoOptionSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: VideoOptionKind,
    pub default: &'static str,
    pub choices: &'static [VideoOptionChoice],
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub visible_when: Option<VideoOptionVisibility>,
}

const X264_PRESETS: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "ultrafast",
        label: "ultrafast",
    },
    VideoOptionChoice {
        value: "superfast",
        label: "superfast",
    },
    VideoOptionChoice {
        value: "veryfast",
        label: "veryfast",
    },
    VideoOptionChoice {
        value: "faster",
        label: "faster",
    },
    VideoOptionChoice {
        value: "fast",
        label: "fast",
    },
    VideoOptionChoice {
        value: "medium",
        label: "medium",
    },
    VideoOptionChoice {
        value: "slow",
        label: "slow",
    },
    VideoOptionChoice {
        value: "slower",
        label: "slower",
    },
    VideoOptionChoice {
        value: "veryslow",
        label: "veryslow",
    },
    VideoOptionChoice {
        value: "placebo",
        label: "placebo",
    },
];
const NVENC_PRESETS: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "p1",
        label: "P1 (fastest)",
    },
    VideoOptionChoice {
        value: "p2",
        label: "P2",
    },
    VideoOptionChoice {
        value: "p3",
        label: "P3",
    },
    VideoOptionChoice {
        value: "p4",
        label: "P4 (default)",
    },
    VideoOptionChoice {
        value: "p5",
        label: "P5",
    },
    VideoOptionChoice {
        value: "p6",
        label: "P6",
    },
    VideoOptionChoice {
        value: "p7",
        label: "P7 (best quality)",
    },
];
const QSV_PRESETS: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "veryfast",
        label: "veryfast",
    },
    VideoOptionChoice {
        value: "faster",
        label: "faster",
    },
    VideoOptionChoice {
        value: "fast",
        label: "fast",
    },
    VideoOptionChoice {
        value: "medium",
        label: "medium",
    },
    VideoOptionChoice {
        value: "slow",
        label: "slow",
    },
    VideoOptionChoice {
        value: "slower",
        label: "slower",
    },
    VideoOptionChoice {
        value: "veryslow",
        label: "veryslow",
    },
];
const VP9_DEADLINES: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "good",
        label: "Good quality",
    },
    VideoOptionChoice {
        value: "realtime",
        label: "Realtime",
    },
];
const ROW_MT: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "auto",
        label: "Automatic",
    },
    VideoOptionChoice {
        value: "1",
        label: "Enabled",
    },
    VideoOptionChoice {
        value: "0",
        label: "Disabled",
    },
];
const X264_RATE_CONTROLS: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "crf",
        label: "CRF",
    },
    VideoOptionChoice {
        value: "cbr",
        label: "CBR",
    },
];
const VBR_CBR: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "vbr",
        label: "VBR",
    },
    VideoOptionChoice {
        value: "cbr",
        label: "CBR",
    },
];
const QSV_RATE_CONTROLS: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "vbr",
        label: "VBR",
    },
    VideoOptionChoice {
        value: "cbr",
        label: "CBR",
    },
    VideoOptionChoice {
        value: "icq",
        label: "ICQ",
    },
];

const MAXRATE: VideoOptionSpec = VideoOptionSpec {
    key: "maxrate",
    label: "Maximum bitrate (kbit/s)",
    kind: VideoOptionKind::Number,
    default: "2400",
    choices: &[],
    minimum: Some(1.0),
    maximum: None,
    visible_when: None,
};
const X264_SETTINGS: &[VideoOptionSpec] = &[
    VideoOptionSpec {
        key: "preset",
        label: "Preset",
        kind: VideoOptionKind::Select,
        default: "faster",
        choices: X264_PRESETS,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "rate_control",
        label: "Rate control",
        kind: VideoOptionKind::Select,
        default: "crf",
        choices: X264_RATE_CONTROLS,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "quality",
        label: "Quality",
        kind: VideoOptionKind::Number,
        default: "23",
        choices: &[],
        minimum: Some(0.0),
        maximum: Some(51.0),
        visible_when: Some(VideoOptionVisibility {
            key: "rate_control",
            value: "crf",
        }),
    },
    MAXRATE,
];
const NVENC_SETTINGS: &[VideoOptionSpec] = &[
    VideoOptionSpec {
        key: "preset",
        label: "Preset",
        kind: VideoOptionKind::Select,
        default: "p4",
        choices: NVENC_PRESETS,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "rate_control",
        label: "Rate control",
        kind: VideoOptionKind::Select,
        default: "vbr",
        choices: VBR_CBR,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "quality",
        label: "Constant quality",
        kind: VideoOptionKind::Number,
        default: "23",
        choices: &[],
        minimum: Some(0.0),
        maximum: Some(51.0),
        visible_when: Some(VideoOptionVisibility {
            key: "rate_control",
            value: "vbr",
        }),
    },
    MAXRATE,
];
const QSV_SETTINGS: &[VideoOptionSpec] = &[
    VideoOptionSpec {
        key: "preset",
        label: "Preset",
        kind: VideoOptionKind::Select,
        default: "faster",
        choices: QSV_PRESETS,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "rate_control",
        label: "Rate control",
        kind: VideoOptionKind::Select,
        default: "vbr",
        choices: QSV_RATE_CONTROLS,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "global_quality",
        label: "Global quality",
        kind: VideoOptionKind::Number,
        default: "23",
        choices: &[],
        minimum: Some(1.0),
        maximum: Some(51.0),
        visible_when: Some(VideoOptionVisibility {
            key: "rate_control",
            value: "icq",
        }),
    },
    MAXRATE,
];
const VP9_SETTINGS: &[VideoOptionSpec] = &[
    VideoOptionSpec {
        key: "rate_control",
        label: "Rate control",
        kind: VideoOptionKind::Select,
        default: "crf",
        choices: X264_RATE_CONTROLS,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "quality",
        label: "Quality",
        kind: VideoOptionKind::Number,
        default: "31",
        choices: &[],
        minimum: Some(0.0),
        maximum: Some(63.0),
        visible_when: Some(VideoOptionVisibility {
            key: "rate_control",
            value: "crf",
        }),
    },
    VideoOptionSpec {
        key: "deadline",
        label: "Encoding mode",
        kind: VideoOptionKind::Select,
        default: "good",
        choices: VP9_DEADLINES,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "cpu-used",
        label: "Speed",
        kind: VideoOptionKind::Number,
        default: "4",
        choices: &[],
        minimum: Some(0.0),
        maximum: Some(8.0),
        visible_when: None,
    },
    VideoOptionSpec {
        key: "row-mt",
        label: "Row multithreading",
        kind: VideoOptionKind::Select,
        default: "auto",
        choices: ROW_MT,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    MAXRATE,
];
const SVT_AV1_PRESETS: &[VideoOptionChoice] = &[
    VideoOptionChoice {
        value: "0",
        label: "0 (slowest)",
    },
    VideoOptionChoice {
        value: "1",
        label: "1",
    },
    VideoOptionChoice {
        value: "2",
        label: "2",
    },
    VideoOptionChoice {
        value: "3",
        label: "3",
    },
    VideoOptionChoice {
        value: "4",
        label: "4",
    },
    VideoOptionChoice {
        value: "5",
        label: "5",
    },
    VideoOptionChoice {
        value: "6",
        label: "6",
    },
    VideoOptionChoice {
        value: "7",
        label: "7",
    },
    VideoOptionChoice {
        value: "8",
        label: "8 (default)",
    },
    VideoOptionChoice {
        value: "9",
        label: "9",
    },
    VideoOptionChoice {
        value: "10",
        label: "10",
    },
    VideoOptionChoice {
        value: "11",
        label: "11",
    },
    VideoOptionChoice {
        value: "12",
        label: "12",
    },
    VideoOptionChoice {
        value: "13",
        label: "13 (fastest)",
    },
];
const SVT_AV1_SETTINGS: &[VideoOptionSpec] = &[
    VideoOptionSpec {
        key: "preset",
        label: "Preset",
        kind: VideoOptionKind::Select,
        default: "8",
        choices: SVT_AV1_PRESETS,
        minimum: None,
        maximum: None,
        visible_when: None,
    },
    VideoOptionSpec {
        key: "quality",
        label: "Quality",
        kind: VideoOptionKind::Number,
        default: "30",
        choices: &[],
        minimum: Some(0.0),
        maximum: Some(63.0),
        visible_when: None,
    },
    MAXRATE,
];
const UNCOMPRESSED_VIDEO_SETTINGS: &[VideoOptionSpec] = &[];
const GENERIC_SETTINGS: &[VideoOptionSpec] = &[MAXRATE];

pub fn video_option_specs(codec: &str) -> &'static [VideoOptionSpec] {
    if !video_codec_uses_bitrate(codec) {
        UNCOMPRESSED_VIDEO_SETTINGS
    } else if codec.contains("x264") || codec.contains("x265") {
        X264_SETTINGS
    } else if codec.ends_with("_nvenc") {
        NVENC_SETTINGS
    } else if codec.ends_with("_qsv") {
        QSV_SETTINGS
    } else if codec == "libvpx-vp9" {
        VP9_SETTINGS
    } else if codec == "libsvtav1" {
        SVT_AV1_SETTINGS
    } else {
        GENERIC_SETTINGS
    }
}

pub fn video_codec_uses_bitrate(codec: &str) -> bool {
    !matches!(
        codec,
        "rawvideo" | "v210" | "r210" | "v308" | "v408" | "v410"
    )
}

pub fn audio_codec_uses_bitrate(codec: &str) -> bool {
    !codec.starts_with("pcm_") && !matches!(codec, "alac" | "flac" | "truehd")
}

pub fn video_option_defaults(codec: &str) -> VideoOptions {
    video_option_specs(codec)
        .iter()
        .map(|setting| (setting.key.to_string(), setting.default.to_string()))
        .collect()
}

pub fn validate_video_options(codec: &str, options: &VideoOptions) -> Result<(), String> {
    let specs = video_option_specs(codec);
    for (key, value) in options {
        let Some(spec) = specs.iter().find(|spec| spec.key == key) else {
            return Err(format!(
                "unsupported video option {key:?} for codec {codec:?}"
            ));
        };
        if !video_option_is_visible(spec, options) {
            continue;
        }
        if spec.kind == VideoOptionKind::Select
            && !spec.choices.iter().any(|choice| choice.value == value)
        {
            return Err(format!(
                "unsupported value {value:?} for video option {key:?}"
            ));
        }
        if spec.kind == VideoOptionKind::Number {
            let number = value
                .parse::<f64>()
                .map_err(|_| format!("video option {key:?} must be a number"))?;
            if !number.is_finite()
                || spec.minimum.is_some_and(|minimum| number < minimum)
                || spec.maximum.is_some_and(|maximum| number > maximum)
            {
                return Err(format!("video option {key:?} is outside its allowed range"));
            }
        }
    }
    for spec in specs {
        if video_option_is_visible(spec, options) && !options.contains_key(spec.key) {
            return Err(format!(
                "missing video option {:?} for codec {codec:?}",
                spec.key
            ));
        }
    }
    Ok(())
}

fn video_option_is_visible(spec: &VideoOptionSpec, options: &VideoOptions) -> bool {
    spec.visible_when.is_none_or(|condition| {
        options
            .get(condition.key)
            .is_some_and(|value| value == condition.value)
    })
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Quiet,
    Panic,
    Fatal,
    Error,
    #[default]
    Warning,
    Info,
    Verbose,
    Debug,
    Trace,
}

impl LogLevel {
    pub(crate) fn as_ffmpeg_level(self) -> FfmpegLevel {
        match self {
            Self::Quiet => FfmpegLevel::Quiet,
            Self::Panic => FfmpegLevel::Panic,
            Self::Fatal => FfmpegLevel::Fatal,
            Self::Error => FfmpegLevel::Error,
            Self::Warning => FfmpegLevel::Warning,
            Self::Info => FfmpegLevel::Info,
            Self::Verbose => FfmpegLevel::Verbose,
            Self::Debug => FfmpegLevel::Debug,
            Self::Trace => FfmpegLevel::Trace,
        }
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(level: &str) -> Result<Self, Self::Err> {
        match level.to_ascii_lowercase().as_str() {
            "quiet" | "off" => Ok(Self::Quiet),
            "panic" => Ok(Self::Panic),
            "fatal" => Ok(Self::Fatal),
            "error" => Ok(Self::Error),
            "warn" | "warning" => Ok(Self::Warning),
            "info" => Ok(Self::Info),
            "verbose" => Ok(Self::Verbose),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            _ => Err(format!("unsupported log level {level:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogoConfig {
    pub path: String,
    pub scale: Option<String>,
    pub opacity: f64,
    pub position: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextConfig {
    pub text: Option<String>,
    pub use_filename: bool,
    pub filename_regex: Option<String>,
    pub font_family: Option<String>,
    pub font_weight: TextWeight,
    pub font_size: f32,
    pub line_spacing: f32,
    pub text_color: RgbaColor,
    pub opacity: f64,
    pub position_x: TextPosition,
    pub position_y: TextPosition,
    pub background: Option<TextBackgroundConfig>,
    pub scroll: TextScroll,
    pub scroll_repeat: i32,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            text: None,
            use_filename: false,
            filename_regex: None,
            font_family: None,
            font_weight: TextWeight::Normal,
            font_size: 48.0,
            line_spacing: 0.0,
            text_color: RgbaColor::opaque(255, 255, 255),
            opacity: 1.0,
            position_x: TextPosition::Pixels(32),
            position_y: TextPosition::Pixels(32),
            background: None,
            scroll: TextScroll::None,
            scroll_repeat: -1,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWeight {
    #[default]
    Normal,
    Semibold,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextPosition {
    Pixels(i32),
    Center,
    End(i32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBackgroundConfig {
    pub color: RgbaColor,
    pub padding: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextScroll {
    None,
    LeftToRight { pixels_per_second: u32 },
    RightToLeft { pixels_per_second: u32 },
}

#[derive(Debug, Clone, Default)]
pub struct TextOverlayState {
    inner: Arc<RwLock<TextOverlayStateInner>>,
}

#[derive(Debug, Clone, Default)]
struct TextOverlayStateInner {
    revision: u64,
    config: Option<TextConfig>,
    start_pts: Option<i64>,
}

impl TextOverlayState {
    pub fn set(&self, config: Option<TextConfig>) {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        inner.revision = inner.revision.wrapping_add(1);
        inner.config = config;
        inner.start_pts = None;
    }

    pub fn clear(&self) {
        self.set(None);
    }

    pub(crate) fn snapshot_at(&self, pts: i64) -> TextOverlaySnapshot {
        // Fast path with a read lock: this is called once per rendered frame,
        // the write lock is only needed right after a new config was set.
        {
            let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
            if inner.config.is_none() || inner.start_pts.is_some() {
                return TextOverlaySnapshot {
                    revision: inner.revision,
                    config: inner.config.clone(),
                    start_pts: inner.start_pts,
                };
            }
        }

        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        if inner.config.is_some() && inner.start_pts.is_none() {
            inner.start_pts = Some(pts);
        }
        TextOverlaySnapshot {
            revision: inner.revision,
            config: inner.config.clone(),
            start_pts: inner.start_pts,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TextOverlaySnapshot {
    pub revision: u64,
    pub config: Option<TextConfig>,
    pub start_pts: Option<i64>,
}

impl OutputConfig {
    pub fn new(width: u32, height: u32, fps: u32, sample_rate: u32) -> Self {
        Self {
            width,
            height,
            desktop_window_size: None,
            desktop_fullscreen: false,
            fps,
            sample_rate,
            video_time_base: Rational(1, fps as i32),
            audio_time_base: Rational(1, sample_rate as i32),
            audio_effects: AudioEffectsControl::default(),
            audio_level_callback: None,
            logo: None,
            text: None,
            text_overlay_state: TextOverlayState::default(),
            stream_type: StreamType::Rtmp,
            stream_format: String::new(),
            video_codec: "libx264".to_string(),
            video_options: video_option_defaults("libx264"),
            audio_codec: "aac".to_string(),
            audio_bitrate: 128_000,
            ffmpeg_log_level: LogLevel::Warning,
            ingest_log_level: LogLevel::Warning,
            ffmpeg_ignore_lines: Vec::new(),
            channel_id: None,
        }
    }

    pub fn with_volume(mut self, volume: f64) -> anyhow::Result<Self> {
        self.audio_effects = AudioEffectsControl::new(volume)?;
        Ok(self)
    }

    pub fn with_audio_effects(mut self, audio_effects: AudioEffectsControl) -> Self {
        self.audio_effects = audio_effects;
        self
    }

    pub fn with_audio_level_callback(mut self, callback: Option<AudioLevelCallback>) -> Self {
        self.audio_level_callback = callback;
        self
    }

    pub fn with_desktop_fullscreen(mut self, fullscreen: bool) -> Self {
        self.desktop_fullscreen = fullscreen;
        self
    }

    pub fn with_logo(mut self, logo: Option<LogoConfig>) -> Self {
        self.logo = logo;
        self
    }

    pub fn with_text(mut self, text: Option<TextConfig>) -> Self {
        self.text = text;
        self
    }

    pub fn with_text_overlay_state(mut self, text_overlay_state: TextOverlayState) -> Self {
        self.text_overlay_state = text_overlay_state;
        self
    }

    pub fn with_stream_type(mut self, stream_type: StreamType) -> Self {
        self.stream_type = stream_type;
        self
    }

    pub fn with_stream_format(mut self, stream_format: String) -> Self {
        self.stream_format = stream_format;
        self
    }

    pub fn with_encoding(
        mut self,
        video_codec: String,
        video_options: VideoOptions,
        audio_codec: String,
        audio_bitrate: u64,
    ) -> Self {
        self.video_codec = video_codec;
        self.video_options = video_options;
        self.audio_codec = audio_codec;
        self.audio_bitrate = audio_bitrate;
        self
    }

    pub fn video_option(&self, key: &str) -> Option<&str> {
        self.video_options.get(key).map(String::as_str)
    }

    pub fn video_maxrate(&self) -> u64 {
        self.video_option("maxrate")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(2_400)
            .saturating_mul(1_000)
    }

    pub fn with_logging(mut self, ffmpeg_log_level: LogLevel, ingest_log_level: LogLevel) -> Self {
        self.ffmpeg_log_level = ffmpeg_log_level;
        self.ingest_log_level = ingest_log_level;
        self
    }

    pub fn with_ffmpeg_ignore_lines(mut self, ignore_lines: Vec<String>) -> Self {
        self.ffmpeg_ignore_lines = ignore_lines;
        self
    }

    pub fn with_channel_id(mut self, channel_id: i32) -> Self {
        self.channel_id = Some(channel_id);
        self
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self::new(1024, 576, 25, 48_000)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSize {
    pub width: u32,
    pub height: u32,
}

impl FromStr for OutputSize {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (width, height) = value
            .split_once(':')
            .or_else(|| value.split_once('x'))
            .ok_or_else(|| "size must use WIDTH:HEIGHT or WIDTHxHEIGHT".to_string())?;
        let width = width
            .parse::<u32>()
            .map_err(|_| "width must be a positive integer".to_string())?;
        let height = height
            .parse::<u32>()
            .map_err(|_| "height must be a positive integer".to_string())?;
        if width == 0 || height == 0 {
            return Err("width and height must be greater than zero".to_string());
        }
        if width % 2 != 0 || height % 2 != 0 {
            return Err("width and height must be even for YUV420 output".to_string());
        }
        Ok(Self { width, height })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        OutputSize, audio_codec_uses_bitrate, validate_video_options, video_codec_uses_bitrate,
        video_option_defaults,
    };

    #[test]
    fn parses_output_size_with_colon() {
        let size = "1280:720".parse::<OutputSize>().unwrap();
        assert_eq!(size.width, 1280);
        assert_eq!(size.height, 720);
    }

    #[test]
    fn parses_output_size_with_x() {
        let size = "1920x1080".parse::<OutputSize>().unwrap();
        assert_eq!(size.width, 1920);
        assert_eq!(size.height, 1080);
    }

    #[test]
    fn rejects_odd_output_size() {
        assert!("1023:576".parse::<OutputSize>().is_err());
        assert!("1024:575".parse::<OutputSize>().is_err());
    }

    #[test]
    fn vp9_options_include_realtime_controls() {
        let options = video_option_defaults("libvpx-vp9");

        assert_eq!(options.get("rate_control").map(String::as_str), Some("crf"));
        assert_eq!(options.get("deadline").map(String::as_str), Some("good"));
        assert_eq!(options.get("cpu-used").map(String::as_str), Some("4"));
        assert_eq!(options.get("row-mt").map(String::as_str), Some("auto"));
        assert!(validate_video_options("libvpx-vp9", &options).is_ok());
    }

    #[test]
    fn svt_av1_options_include_preset_and_quality() {
        let options = video_option_defaults("libsvtav1");

        assert_eq!(options.get("preset").map(String::as_str), Some("8"));
        assert_eq!(options.get("quality").map(String::as_str), Some("30"));
        assert!(validate_video_options("libsvtav1", &options).is_ok());
    }

    #[test]
    fn uncompressed_codecs_do_not_use_bitrate_settings() {
        assert!(video_option_defaults("rawvideo").is_empty());
        assert!(!video_codec_uses_bitrate("rawvideo"));
        assert!(!audio_codec_uses_bitrate("pcm_s16le"));
        assert!(!audio_codec_uses_bitrate("flac"));
        assert!(audio_codec_uses_bitrate("aac"));
    }

    #[test]
    fn qsv_icq_uses_global_quality() {
        let mut options = video_option_defaults("h264_qsv");
        options.insert("rate_control".to_string(), "icq".to_string());

        assert_eq!(
            options.get("global_quality").map(String::as_str),
            Some("23")
        );
        assert!(validate_video_options("h264_qsv", &options).is_ok());
    }
}
