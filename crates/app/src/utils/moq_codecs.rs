//! Validated MoQ codec combinations, independent of encoder names.
// Direct publishing also has parsers for other codecs. Advertise those only after
// verifying their encoder initialization and browser playback end to end.
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
    fn publisher_capabilities_are_codec_ids_not_encoder_names() {
        assert!(supports_video("h264"));
        assert!(supports_video("hevc"));
        assert!(!supports_video("av1"));
        assert!(supports_audio("aac"));
        assert!(!supports_audio("libfdk_aac"));
    }
}
