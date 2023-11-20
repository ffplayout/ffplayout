use std::io;

use derive_more::Display;

#[derive(Debug, Display)]
pub enum ProcError {
    #[display(fmt = "Failed to spawn ffmpeg/ffprobe. {}", _0)]
    CommandSpawn(io::Error),
    #[display(fmt = "Failed to read data from ffmpeg/ffprobe. {}", _0)]
    IO(io::Error),
    #[display(fmt = "{}", _0)]
    Custom(String),
    #[display(fmt = "Regex compile error {}", _0)]
    Regex(String),
}

impl From<std::io::Error> for ProcError {
    fn from(err: std::io::Error) -> Self {
        Self::CommandSpawn(err)
    }
}

impl From<regex::Error> for ProcError {
    fn from(err: regex::Error) -> Self {
        Self::Regex(err.to_string())
    }
}
