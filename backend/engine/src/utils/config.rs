use std::str::FromStr;

use ffmpeg_next::Rational;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsVariant {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub video_bitrate: u64,
    pub audio_bitrate: u64,
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

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub sample_rate: u32,
    pub video_time_base: Rational,
    pub audio_time_base: Rational,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 576,
            fps: 25,
            sample_rate: 48_000,
            video_time_base: Rational(1, 25),
            audio_time_base: Rational(1, 48_000),
        }
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
