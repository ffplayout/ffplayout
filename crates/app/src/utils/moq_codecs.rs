//! Codec IDs supported by the current MoQ MPEG-TS importer, independent of encoder names.
// Keep transport support here, never in the database schema. AV1 requires a compatible
// import/container path before it can be advertised, even if FFmpeg can encode it.
pub(crate) fn supports_video(codec_id: &str) -> bool {
    matches!(codec_id, "h264" | "hevc")
}

pub(crate) fn supports_audio(codec_id: &str) -> bool {
    codec_id == "aac"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importer_capabilities_are_codec_ids_not_encoder_names() {
        assert!(supports_video("h264"));
        assert!(supports_video("hevc"));
        assert!(!supports_video("av1"));
        assert!(supports_audio("aac"));
        assert!(!supports_audio("libfdk_aac"));
    }
}
