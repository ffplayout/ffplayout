use std::sync::OnceLock;

use ffmpeg_next::ffi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FfmpegFeatures {
    pub(crate) hls_subtitle_name: bool,
}

impl FfmpegFeatures {
    pub(crate) fn detect() -> Self {
        let avformat = linked_avformat_version();

        Self {
            hls_subtitle_name: avformat >= AvVersion::new(61, 7, 100),
        }
    }
}

pub(crate) fn ffmpeg_features() -> FfmpegFeatures {
    *FEATURES.get_or_init(FfmpegFeatures::detect)
}

static FEATURES: OnceLock<FfmpegFeatures> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct AvVersion {
    major: u32,
    minor: u32,
    micro: u32,
}

impl AvVersion {
    const fn new(major: u32, minor: u32, micro: u32) -> Self {
        Self {
            major,
            minor,
            micro,
        }
    }
}

fn linked_avformat_version() -> AvVersion {
    let version = unsafe { ffi::avformat_version() };
    AvVersion {
        major: version >> 16,
        minor: (version >> 8) & 0xff,
        micro: version & 0xff,
    }
}

#[cfg(test)]
mod tests {
    use super::{AvVersion, FfmpegFeatures};

    #[test]
    fn detects_hls_subtitle_name_support_from_avformat_version() {
        assert!(
            !FfmpegFeatures {
                hls_subtitle_name: AvVersion::new(61, 6, 100) >= AvVersion::new(61, 7, 100),
            }
            .hls_subtitle_name
        );
        assert!(
            FfmpegFeatures {
                hls_subtitle_name: AvVersion::new(61, 7, 100) >= AvVersion::new(61, 7, 100),
            }
            .hls_subtitle_name
        );
    }
}
