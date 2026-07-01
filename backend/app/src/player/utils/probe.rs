use log::*;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use tokio::process;

use crate::utils::errors::ProcessError;

pub async fn ffprobe(path: impl AsRef<std::path::Path>) -> Result<FfProbe, FfProbeError> {
    ffprobe_config(path).await
}

pub async fn ffprobe_config(path: impl AsRef<std::path::Path>) -> Result<FfProbe, FfProbeError> {
    let path = path.as_ref();

    let mut cmd = process::Command::new("ffprobe");

    cmd.args([
        "-v",
        "error",
        "-show_format",
        "-show_streams",
        "-print_format",
        "json",
    ]);

    cmd.arg(path);

    // Prevent CMD popup on Windows.
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);

    let out = cmd.output().await.map_err(FfProbeError::Io)?;

    if !out.status.success() {
        return Err(FfProbeError::Status(out));
    }

    serde_json::from_slice::<FfProbe>(&out.stdout).map_err(FfProbeError::Deserialize)
}

#[derive(Debug)]
#[non_exhaustive]
pub enum FfProbeError {
    Io(std::io::Error),
    Status(std::process::Output),
    Deserialize(serde_json::Error),
}

impl std::fmt::Display for FfProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfProbeError::Io(e) => e.fmt(f),
            FfProbeError::Status(o) => {
                write!(
                    f,
                    "ffprobe exited with {} <span class=\"log-addr\">{}</span>",
                    o.status,
                    String::from_utf8_lossy(&o.stderr).trim()
                )
            }
            FfProbeError::Deserialize(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for FfProbeError {}

#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FfProbe {
    pub streams: Vec<Stream>,
    pub format: Format,
}

#[serde_as]
#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stream {
    pub index: i64,
    pub codec_name: Option<String>,
    pub display_aspect_ratio: Option<String>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub nb_frames: Option<i64>,
    pub codec_type: Option<String>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub sample_rate: Option<i64>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub bit_rate: Option<i64>,
    pub channels: Option<i64>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub duration: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub r_frame_rate: String,
    pub field_order: Option<String>,
}

#[serde_as]
#[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Format {
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub duration: Option<f64>,
    pub nb_streams: i64,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub size: Option<i64>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub bit_rate: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MediaFormat {
    pub duration: Option<f64>,
    pub nb_streams: i64,
    pub size: i64,
}

impl MediaFormat {
    pub fn new(format: Format) -> Self {
        Self {
            duration: format.duration,
            nb_streams: format.nb_streams,
            size: format.size.unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioStream {
    pub channels: i64,
    pub codec_name: Option<String>,
    pub duration: Option<f64>,
    pub sample_rate: Option<i64>,
}

impl AudioStream {
    pub fn new(stream: Stream) -> Self {
        Self {
            channels: stream.channels.unwrap_or(2),
            codec_name: stream.codec_name,
            duration: stream.duration,
            sample_rate: stream.sample_rate,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VideoStream {
    pub codec_name: Option<String>,
    pub aspect_ratio: Option<String>,
    pub nb_frames: Option<i64>,
    pub duration: Option<f64>,
    pub bit_rate: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frame_rate: String,
    pub field_order: Option<String>,
}

impl VideoStream {
    pub fn new(stream: Stream) -> Self {
        Self {
            codec_name: stream.codec_name,
            aspect_ratio: stream.display_aspect_ratio,
            nb_frames: stream.nb_frames,
            duration: stream.duration,
            bit_rate: stream.bit_rate,
            width: stream.width,
            height: stream.height,
            frame_rate: stream.r_frame_rate,
            field_order: stream.field_order,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct MediaProbe {
    pub format: MediaFormat,
    pub audio: Vec<AudioStream>,
    pub video: Vec<VideoStream>,
}

impl MediaProbe {
    pub async fn new(
        input: impl AsRef<std::path::Path> + std::marker::Copy,
    ) -> Result<Self, ProcessError> {
        let mut a_stream = vec![];
        let mut v_stream = vec![];

        let probe = ffprobe(input).await?;
        for stream in probe.streams {
            let cp_stream = stream.clone();

            if let Some(c_type) = cp_stream.codec_type {
                match c_type.as_str() {
                    "audio" => a_stream.push(AudioStream::new(stream)),
                    "video" => v_stream.push(VideoStream::new(stream)),
                    _ => {}
                }
            } else {
                error!("No codec type found for stream: {stream:?}");
            }
        }

        Ok(Self {
            format: MediaFormat::new(probe.format),
            audio: a_stream,
            video: v_stream,
        })
    }

    pub fn format_duration(self) -> f64 {
        self.format.duration.unwrap_or_default()
    }

    pub fn aspect(&self) -> f64 {
        let mut asp = self
            .video
            .first()
            .and_then(|v| v.aspect_ratio.as_ref())
            .and_then(|ratio| ratio.split_once(':'))
            .and_then(|(w, h)| {
                let width = w.parse::<f64>().ok()?;
                let height = h.parse::<f64>().ok()?;
                Some(width / height)
            })
            .unwrap_or(0.0);

        if asp == 0.0 {
            asp = self
                .video
                .first()
                .and_then(|v| v.width.zip(v.height))
                .map(|(width, height)| width as f64 / height as f64)
                .unwrap_or(0.0);
        }

        asp
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use path_clean::PathClean;

    use super::*;

    #[tokio::test]
    async fn print_media_probe() {
        let path = env::current_dir()
            .unwrap()
            .join("../test/assets/media/with_audio.mp4")
            .clean();

        let probe = MediaProbe::new(&path).await;

        println!("{probe:#?}");
    }
}
