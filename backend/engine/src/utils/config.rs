use std::str::FromStr;

use ffmpeg_next::{Rational, util::log::Level as FfmpegLevel};

use crate::AudioEffectsControl;

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
    pub fps: u32,
    pub sample_rate: u32,
    pub video_time_base: Rational,
    pub audio_time_base: Rational,
    pub audio_effects: AudioEffectsControl,
    pub logo: Option<LogoConfig>,
    pub video_preset: String,
    pub rate_control: RateControl,
    pub video_quality: u8,
    pub video_maxrate: u64,
    pub audio_bitrate: u64,
    pub ffmpeg_log_level: LogLevel,
    pub ingest_log_level: LogLevel,
    pub channel_id: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateControl {
    Crf,
    Cbr,
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

impl OutputConfig {
    pub fn new(width: u32, height: u32, fps: u32, sample_rate: u32) -> Self {
        Self {
            width,
            height,
            fps,
            sample_rate,
            video_time_base: Rational(1, fps as i32),
            audio_time_base: Rational(1, sample_rate as i32),
            audio_effects: AudioEffectsControl::default(),
            logo: None,
            video_preset: "faster".to_string(),
            rate_control: RateControl::Crf,
            video_quality: 23,
            video_maxrate: 2_400_000,
            audio_bitrate: 128_000,
            ffmpeg_log_level: LogLevel::Warning,
            ingest_log_level: LogLevel::Warning,
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

    pub fn with_logo(mut self, logo: Option<LogoConfig>) -> Self {
        self.logo = logo;
        self
    }

    pub fn with_encoding(
        mut self,
        preset: String,
        rate_control: RateControl,
        quality: u8,
        maxrate: u64,
        audio_bitrate: u64,
    ) -> Self {
        self.video_preset = preset;
        self.rate_control = rate_control;
        self.video_quality = quality;
        self.video_maxrate = maxrate;
        self.audio_bitrate = audio_bitrate;
        self
    }

    pub fn with_logging(mut self, ffmpeg_log_level: LogLevel, ingest_log_level: LogLevel) -> Self {
        self.ffmpeg_log_level = ffmpeg_log_level;
        self.ingest_log_level = ingest_log_level;
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
    use super::OutputSize;

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
}
