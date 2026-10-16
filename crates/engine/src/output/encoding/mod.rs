//! Codec ownership and encoding are independent of delivery destinations.
pub(super) mod audio;
pub(super) mod video;
mod workers;
pub(super) use audio::AudioOutputStream;
pub(super) use video::VideoOutputStream;
pub(super) use workers::{EncodeFrame, EncodedPacket, EncodingWorkers};

mod profile;
pub(super) use profile::EncodingProfile;
