use std::path::{Path, PathBuf};

use anyhow::Result;

/// Encoded packet metadata on a common, continuous microsecond clock.
/// Each rendition owns adjacent video/audio stream indices (0/1, 2/3, ...).
/// The payload stays owned by the caller's container implementation.
pub struct MediaPacket<T> {
    pub stream: usize,
    pub pts_us: i64,
    pub dts_us: i64,
    pub duration_us: i64,
    pub keyframe: bool,
    pub bytes: usize,
    pub payload: T,
}

pub trait SegmentWriter<T> {
    /// Write, close, then atomically publish a new segment. Never overwrite an
    /// existing resource. All packets retain their common presentation clock.
    fn write_segment(
        &mut self,
        path: &Path,
        packets: &[MediaPacket<T>],
        rendition: usize,
    ) -> Result<()>;
}

pub struct RenditionConfig {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Peak aggregate bitrate in bits per second, including container overhead.
    pub bandwidth: u64,
    /// RFC 6381 codec list; omit when it cannot be determined reliably.
    pub codecs: String,
}

#[derive(Clone)]
pub struct Subtitle {
    pub name: String,
    pub language: String,
    pub default: bool,
}

/// Already mapped to the program timeline by the caller.
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

pub struct Config {
    pub directory: PathBuf,
    pub renditions: Vec<RenditionConfig>,
    pub subtitle: Option<Subtitle>,
    pub master_playlist: Option<String>,
    pub segment_seconds: u32,
    pub list_size: u32,
    /// Number of unreferenced segments retained per track; normally 1.
    pub delete_threshold: u32,
    pub program_date_time: bool,
}
