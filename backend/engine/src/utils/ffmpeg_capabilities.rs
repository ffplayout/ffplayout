use std::{
    ffi::{CStr, CString, c_void},
    ptr,
    sync::OnceLock,
};

use ffmpeg_next::{codec, ffi, format::Pixel, media};

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
            .filter(|encoder| output_muxer_supports_codec(target, muxer, encoder))
            .filter(|encoder| {
                encoder.media_type != FfmpegMediaType::Video || encoder.engine_video_format
            })
            .filter(|encoder| output_supports_codec(target, encoder))
            .cloned()
            .collect()
    }

    pub fn has_muxer(&self, target: FfmpegOutputTarget) -> bool {
        muxer_for_target(target)
            .is_some_and(|muxer_name| self.muxers.iter().any(|muxer| muxer.name == muxer_name))
    }

    pub fn usable_codecs(&self, media_type: FfmpegMediaType) -> Vec<FfmpegCodec> {
        self.encoders
            .iter()
            .filter(|encoder| encoder.media_type == media_type)
            .filter(|encoder| {
                encoder.media_type != FfmpegMediaType::Video || encoder.engine_video_format
            })
            .cloned()
            .collect()
    }

    pub fn has_muxer_named(&self, muxer_name: &str) -> bool {
        muxer_for_name(muxer_name).is_some()
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
    engine_video_format: bool,
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
    Srt,
    Udp,
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
    let version = ffmpeg_next::format::version();
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
        let codec_ptr = unsafe { ffi::av_codec_iterate(&mut opaque) };
        if codec_ptr.is_null() {
            break;
        }
        let codec = unsafe { codec::codec::Codec::wrap(codec_ptr) };
        if !codec.is_encoder() {
            continue;
        }

        let media_type = match codec.medium() {
            media::Type::Audio => FfmpegMediaType::Audio,
            media::Type::Video => FfmpegMediaType::Video,
            media::Type::Subtitle => FfmpegMediaType::Subtitle,
            _ => continue,
        };
        let name = codec.name().to_string();
        if name.is_empty() {
            continue;
        }

        let codec_id_raw = codec.id().into();
        encoders.push(FfmpegCodec {
            display_name: codec.description().to_string(),
            codec_id: codec_id_name(codec_id_raw),
            media_type,
            hardware: is_hardware_encoder(codec, &name),
            wrapper: codec_wrapper_name(codec),
            engine_video_format: media_type != FfmpegMediaType::Video
                || video_encoder_supports_engine_format(codec, &name),
            codec_id_raw,
            name,
        });
    }

    encoders.sort_by(|left, right| left.name.cmp(&right.name));
    encoders
}

fn detect_muxers() -> Vec<FfmpegMuxer> {
    ["hls", "flv", "mpegts"]
        .into_iter()
        .filter_map(|name| {
            let muxer = muxer_for_name(name)?;
            Some(FfmpegMuxer {
                name: name.to_string(),
                long_name: muxer.description().to_string(),
            })
        })
        .collect()
}

fn muxer_supports_codec(muxer_name: &str, codec_id: ffi::AVCodecID) -> bool {
    let Some(muxer) = muxer_for_name(muxer_name) else {
        return false;
    };

    unsafe { ffi::avformat_query_codec(muxer.as_ptr(), codec_id, ffi::FF_COMPLIANCE_NORMAL) > 0 }
}

fn output_muxer_supports_codec(
    target: FfmpegOutputTarget,
    muxer_name: &str,
    codec: &FfmpegCodec,
) -> bool {
    if matches!(target, FfmpegOutputTarget::Srt | FfmpegOutputTarget::Udp) {
        return mpegts_output_supports_codec(codec);
    }

    muxer_supports_codec(muxer_name, codec.codec_id_raw)
}

fn muxer_for_target(target: FfmpegOutputTarget) -> Option<&'static str> {
    match target {
        FfmpegOutputTarget::Hls => Some("hls"),
        FfmpegOutputTarget::Rtmp => Some("flv"),
        FfmpegOutputTarget::Srt | FfmpegOutputTarget::Udp => Some("mpegts"),
    }
}

fn output_supports_codec(target: FfmpegOutputTarget, codec: &FfmpegCodec) -> bool {
    if target != FfmpegOutputTarget::Rtmp {
        return true;
    }

    match codec.media_type {
        FfmpegMediaType::Audio => matches!(codec.codec_id.as_str(), "aac" | "mp3"),
        FfmpegMediaType::Video => {
            !matches!(
                codec.name.as_str(),
                "flv" | "h263" | "h263p" | "libxvid" | "mpeg4"
            ) && !matches!(codec.codec_id.as_str(), "h263" | "mpeg4")
        }
        FfmpegMediaType::Subtitle => true,
    }
}

fn mpegts_output_supports_codec(codec: &FfmpegCodec) -> bool {
    match codec.media_type {
        FfmpegMediaType::Audio => matches!(
            codec.codec_id.as_str(),
            "aac" | "mp2" | "mp3" | "ac3" | "eac3" | "opus"
        ),
        FfmpegMediaType::Video => matches!(
            codec.codec_id.as_str(),
            "h264" | "hevc" | "mpeg2video" | "mpeg4"
        ),
        FfmpegMediaType::Subtitle => false,
    }
}

fn muxer_for_name(name: &str) -> Option<ffmpeg_next::format::format::Output> {
    let name = CString::new(name).ok()?;
    let muxer = unsafe { ffi::av_guess_format(name.as_ptr(), ptr::null(), ptr::null()) };
    (!muxer.is_null())
        .then(|| unsafe { ffmpeg_next::format::format::Output::wrap(muxer.cast_mut()) })
}

fn codec_id_name(codec_id: ffi::AVCodecID) -> String {
    codec::Id::from(codec_id).name().to_string()
}

fn is_hardware_encoder(codec: codec::codec::Codec, name: &str) -> bool {
    let capabilities = unsafe { (*codec.as_ptr()).capabilities };
    if capabilities & ffi::AV_CODEC_CAP_HARDWARE as i32 != 0 {
        return true;
    }

    matches!(
        name.rsplit_once('_').map(|(_, suffix)| suffix),
        Some(
            "amf" | "cuda" | "mediacodec" | "nvenc" | "qsv" | "vaapi" | "v4l2m2m" | "videotoolbox"
        )
    )
}

fn video_encoder_supports_engine_format(codec: codec::codec::Codec, name: &str) -> bool {
    let Ok(video) = codec.video() else {
        return false;
    };
    let Some(formats) = video.formats() else {
        return true;
    };

    for pixel in formats {
        if pixel == Pixel::YUV420P
            || (is_qsv_encoder(name) && pixel == Pixel::NV12)
            || (is_vaapi_encoder(name) && pixel == Pixel::VAAPI)
        {
            return true;
        }
    }

    false
}

fn is_qsv_encoder(name: &str) -> bool {
    name.ends_with("_qsv")
}

fn is_vaapi_encoder(name: &str) -> bool {
    name.ends_with("_vaapi")
}

fn codec_wrapper_name(codec: codec::codec::Codec) -> Option<String> {
    let wrapper_name = unsafe { (*codec.as_ptr()).wrapper_name };
    non_empty_c_string(wrapper_name)
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
    fn excludes_rgb_only_video_encoders() {
        let capabilities = FfmpegCapabilities::detect();
        if !capabilities
            .encoders
            .iter()
            .any(|codec| codec.name == "libx264rgb")
        {
            return;
        }

        assert!(
            !capabilities
                .video_codecs_for(FfmpegOutputTarget::Hls)
                .iter()
                .any(|codec| codec.name == "libx264rgb")
        );
        assert!(
            !capabilities
                .video_codecs_for(FfmpegOutputTarget::Rtmp)
                .iter()
                .any(|codec| codec.name == "libx264rgb")
        );
    }

    #[test]
    fn excludes_legacy_mpeg4_family_from_rtmp_video_codecs() {
        let capabilities = FfmpegCapabilities::detect();
        let rtmp_video = capabilities.video_codecs_for(FfmpegOutputTarget::Rtmp);

        for codec in ["h263", "h263p", "libxvid", "mpeg4"] {
            assert!(
                !rtmp_video.iter().any(|item| item.name == codec),
                "RTMP video codecs should not include {codec}: {rtmp_video:#?}"
            );
        }
    }

    #[test]
    fn limits_rtmp_audio_codecs_to_aac_and_mp3() {
        let capabilities = FfmpegCapabilities::detect();
        let rtmp_audio = capabilities.audio_codecs_for(FfmpegOutputTarget::Rtmp);

        assert!(
            rtmp_audio
                .iter()
                .all(|codec| matches!(codec.codec_id.as_str(), "aac" | "mp3")),
            "RTMP audio codecs should only include AAC/MP3 encoders: {rtmp_audio:#?}"
        );
        assert!(
            !rtmp_audio.iter().any(|codec| codec.name == "pcm_s16le"),
            "RTMP audio codecs should not include pcm_s16le: {rtmp_audio:#?}"
        );
    }

    #[test]
    fn mpegts_stream_targets_include_common_codecs() {
        let capabilities = FfmpegCapabilities::detect();
        let srt_video = capabilities.video_codecs_for(FfmpegOutputTarget::Srt);
        let srt_audio = capabilities.audio_codecs_for(FfmpegOutputTarget::Srt);
        let udp_video = capabilities.video_codecs_for(FfmpegOutputTarget::Udp);
        let udp_audio = capabilities.audio_codecs_for(FfmpegOutputTarget::Udp);

        assert!(
            srt_video.iter().any(|codec| codec.codec_id == "h264"),
            "SRT/MPEG-TS video codecs should include H.264 encoders: {srt_video:#?}"
        );
        assert!(
            srt_audio.iter().any(|codec| codec.codec_id == "aac"),
            "SRT/MPEG-TS audio codecs should include AAC encoders: {srt_audio:#?}"
        );
        assert!(
            udp_video.iter().any(|codec| codec.codec_id == "h264"),
            "UDP/MPEG-TS video codecs should include H.264 encoders: {udp_video:#?}"
        );
        assert!(
            udp_audio.iter().any(|codec| codec.codec_id == "aac"),
            "UDP/MPEG-TS audio codecs should include AAC encoders: {udp_audio:#?}"
        );
    }

    #[test]
    fn cached_capabilities_are_available() {
        let capabilities = ffmpeg_capabilities();
        assert!(capabilities.has_muxer(FfmpegOutputTarget::Hls));
    }
}
