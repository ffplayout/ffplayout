use std::{
    ffi::{CStr, CString, c_void},
    ptr,
    sync::OnceLock,
};

use ffmpeg_next::ffi;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegCapabilities {
    pub features: FfmpegFeatureSet,
    pub encoders: Vec<FfmpegCodec>,
    pub muxers: Vec<FfmpegMuxer>,
}

impl FfmpegCapabilities {
    pub fn detect() -> Self {
        ffmpeg_next::init().ok();
        let avformat = linked_avformat_version();
        let encoders = detect_encoders();
        let muxers = detect_muxers();

        Self {
            features: FfmpegFeatureSet {
                hls_subtitle_name: supports_hls_subtitle_name(avformat),
            },
            encoders,
            muxers,
        }
    }

    pub fn audio_codecs_for(&self, target: FfmpegOutputTarget) -> Vec<FfmpegCodec> {
        self.codecs_for(target, FfmpegMediaType::Audio)
    }

    pub fn video_codecs_for(&self, target: FfmpegOutputTarget) -> Vec<FfmpegCodec> {
        self.codecs_for(target, FfmpegMediaType::Video)
    }

    pub fn codecs_for(
        &self,
        target: FfmpegOutputTarget,
        media_type: FfmpegMediaType,
    ) -> Vec<FfmpegCodec> {
        let Some(muxer) = muxer_for_target(target) else {
            return Vec::new();
        };

        self.encoders
            .iter()
            .filter(|encoder| encoder.media_type == media_type)
            .filter(|encoder| muxer_supports_codec(muxer, encoder.codec_id_raw))
            .cloned()
            .collect()
    }

    pub fn has_muxer(&self, target: FfmpegOutputTarget) -> bool {
        muxer_for_target(target)
            .is_some_and(|muxer_name| self.muxers.iter().any(|muxer| muxer.name == muxer_name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfmpegFeatureSet {
    pub hls_subtitle_name: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegCodec {
    pub name: String,
    pub display_name: String,
    pub codec_id: String,
    pub media_type: FfmpegMediaType,
    pub hardware: bool,
    pub wrapper: Option<String>,
    codec_id_raw: ffi::AVCodecID,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegMuxer {
    pub name: String,
    pub long_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegOutputTarget {
    Hls,
    Rtmp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfmpegMediaType {
    Audio,
    Video,
    Subtitle,
}

pub fn ffmpeg_capabilities() -> &'static FfmpegCapabilities {
    CAPABILITIES.get_or_init(FfmpegCapabilities::detect)
}

static CAPABILITIES: OnceLock<FfmpegCapabilities> = OnceLock::new();

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

fn supports_hls_subtitle_name(avformat: AvVersion) -> bool {
    avformat >= AvVersion::new(61, 9, 100)
}

fn detect_encoders() -> Vec<FfmpegCodec> {
    let mut encoders = Vec::new();
    let mut opaque: *mut c_void = ptr::null_mut();

    loop {
        let codec = unsafe { ffi::av_codec_iterate(&mut opaque) };
        if codec.is_null() {
            break;
        }
        if unsafe { ffi::av_codec_is_encoder(codec) } == 0 {
            continue;
        }

        let media_type = match unsafe { (*codec).type_ } {
            ffi::AVMediaType::AVMEDIA_TYPE_AUDIO => FfmpegMediaType::Audio,
            ffi::AVMediaType::AVMEDIA_TYPE_VIDEO => FfmpegMediaType::Video,
            ffi::AVMediaType::AVMEDIA_TYPE_SUBTITLE => FfmpegMediaType::Subtitle,
            _ => continue,
        };
        let name = c_string(unsafe { (*codec).name });
        if name.is_empty() {
            continue;
        }

        let codec_id_raw = unsafe { (*codec).id };
        encoders.push(FfmpegCodec {
            name,
            display_name: c_string(unsafe { (*codec).long_name }),
            codec_id: codec_id_name(codec_id_raw),
            media_type,
            hardware: is_hardware_encoder(codec),
            wrapper: non_empty_c_string(unsafe { (*codec).wrapper_name }),
            codec_id_raw,
        });
    }

    encoders.sort_by(|left, right| left.name.cmp(&right.name));
    encoders
}

fn detect_muxers() -> Vec<FfmpegMuxer> {
    ["hls", "flv"]
        .into_iter()
        .filter_map(|name| {
            let muxer = muxer_for_name(name)?;
            Some(FfmpegMuxer {
                name: name.to_string(),
                long_name: c_string(unsafe { (*muxer).long_name }),
            })
        })
        .collect()
}

fn muxer_supports_codec(muxer_name: &str, codec_id: ffi::AVCodecID) -> bool {
    let Some(muxer) = muxer_for_name(muxer_name) else {
        return false;
    };

    unsafe { ffi::avformat_query_codec(muxer, codec_id, ffi::FF_COMPLIANCE_NORMAL) > 0 }
}

fn muxer_for_target(target: FfmpegOutputTarget) -> Option<&'static str> {
    match target {
        FfmpegOutputTarget::Hls => Some("hls"),
        FfmpegOutputTarget::Rtmp => Some("flv"),
    }
}

fn muxer_for_name(name: &str) -> Option<*const ffi::AVOutputFormat> {
    let name = CString::new(name).ok()?;
    let muxer = unsafe { ffi::av_guess_format(name.as_ptr(), ptr::null(), ptr::null()) };
    (!muxer.is_null()).then_some(muxer)
}

fn codec_id_name(codec_id: ffi::AVCodecID) -> String {
    c_string(unsafe { ffi::avcodec_get_name(codec_id) })
}

fn is_hardware_encoder(codec: *const ffi::AVCodec) -> bool {
    let capabilities = unsafe { (*codec).capabilities };
    if capabilities & ffi::AV_CODEC_CAP_HARDWARE as i32 != 0 {
        return true;
    }

    let name = c_string(unsafe { (*codec).name });
    matches!(
        name.rsplit_once('_').map(|(_, suffix)| suffix),
        Some(
            "amf" | "cuda" | "mediacodec" | "nvenc" | "qsv" | "vaapi" | "v4l2m2m" | "videotoolbox"
        )
    )
}

fn c_string(value: *const std::ffi::c_char) -> String {
    non_empty_c_string(value).unwrap_or_default()
}

fn non_empty_c_string(value: *const std::ffi::c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }

    let value = unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::{
        AvVersion, FfmpegCapabilities, FfmpegMediaType, FfmpegOutputTarget, ffmpeg_capabilities,
        supports_hls_subtitle_name,
    };

    #[test]
    fn detects_hls_subtitle_name_support_from_avformat_version() {
        assert!(!supports_hls_subtitle_name(AvVersion::new(61, 7, 100)));
        assert!(supports_hls_subtitle_name(AvVersion::new(61, 9, 100)));
    }

    #[test]
    fn reports_common_hls_and_rtmp_codecs() {
        let capabilities = FfmpegCapabilities::detect();
        let hls_audio = capabilities.audio_codecs_for(FfmpegOutputTarget::Hls);
        let rtmp_video = capabilities.video_codecs_for(FfmpegOutputTarget::Rtmp);

        assert!(
            hls_audio
                .iter()
                .any(|codec| codec.codec_id == "aac" && codec.media_type == FfmpegMediaType::Audio),
            "expected at least one HLS-compatible AAC encoder, got {hls_audio:#?}"
        );
        assert!(
            rtmp_video
                .iter()
                .any(|codec| codec.codec_id == "h264" && codec.media_type == FfmpegMediaType::Video),
            "expected at least one RTMP-compatible H.264 encoder, got {rtmp_video:#?}"
        );
    }

    #[test]
    fn cached_capabilities_are_available() {
        let capabilities = ffmpeg_capabilities();
        assert!(capabilities.has_muxer(FfmpegOutputTarget::Hls));
    }
}
