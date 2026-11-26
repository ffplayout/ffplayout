//! HLS orchestration independent of codecs and runtimes.
//! Callers supply encoded packets and a container writer. A single packager
//! owns an output's manifests; the caller must enforce exclusive file ownership.
mod manifest;
mod packager;
pub mod retention;

pub const SHARED_AUDIO_NAME: &str = "_ffplayout_audio";
pub const SHARED_AUDIO_MARKER: &str = "# ffplayout-audio: shared";

pub use packager::{Config, Cue, MediaPacket, Packager, RenditionConfig, SegmentWriter, Subtitle};
