use std::io;

use derive_more::Display;
use ffprobe::FfProbeError;

#[derive(Debug, Display)]
pub enum ProcError {
    #[display(fmt = "Failed to spawn ffmpeg/ffprobe. {}", _0)]
    CommandSpawn(io::Error),
    #[display(fmt = "IO Error {}", _0)]
    IO(io::Error),
    #[display(fmt = "{}", _0)]
    Custom(String),
    #[display(fmt = "{}", _0)]
    Ffprobe(FfProbeError),
    #[display(fmt = "Regex compile error {}", _0)]
    Regex(String),
    #[display(fmt = "Thread error {}", _0)]
    Thread(String),
}

impl From<std::io::Error> for ProcError {
    fn from(err: std::io::Error) -> Self {
        Self::CommandSpawn(err)
    }
}

impl From<FfProbeError> for ProcError {
    fn from(err: FfProbeError) -> Self {
        Self::Ffprobe(err)
    }
}

impl From<regex::Error> for ProcError {
    fn from(err: regex::Error) -> Self {
        Self::Regex(err.to_string())
    }
}

impl From<log::SetLoggerError> for ProcError {
    fn from(err: log::SetLoggerError) -> Self {
        Self::Custom(err.to_string())
    }
}

impl From<serde_json::Error> for ProcError {
    fn from(err: serde_json::Error) -> Self {
        Self::Custom(err.to_string())
    }
}

impl From<Box<dyn std::any::Any + std::marker::Send>> for ProcError {
    fn from(err: Box<dyn std::any::Any + std::marker::Send>) -> Self {
        Self::Thread(format!("{err:?}"))
    }
}
