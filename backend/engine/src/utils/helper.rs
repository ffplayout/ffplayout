use ffmpeg_next::{Dictionary, Error as FfmpegError, format};

/// Timeout for blocking network I/O (`rw_timeout` is in microseconds). Without
/// it a stalled TCP connection blocks the playout worker indefinitely; the
/// skip/abort flags are only checked between packets and never reach a thread
/// that is stuck inside a single read or write syscall.
const NETWORK_IO_TIMEOUT_US: &str = "10000000";

pub fn even(value: u32) -> u32 {
    value & !1
}

pub(crate) fn is_network_url(path: &str) -> bool {
    path.split_once("://")
        .is_some_and(|(scheme, _)| !scheme.eq_ignore_ascii_case("file"))
}

/// Returns whether an input URL is an unbounded live transport rather than a
/// seekable media resource. HTTP(S) deliberately remains excluded because it
/// may refer to either VOD or a live manifest and needs demuxer-level handling.
pub fn is_live_input(path: &str) -> bool {
    let Some((scheme, _)) = path.split_once("://") else {
        return false;
    };

    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "rtmp" | "rtmps" | "rtp" | "rtsp" | "srt" | "udp" | "tcp" | "rist"
    )
}

pub(crate) fn network_io_options() -> Dictionary<'static> {
    let mut options = Dictionary::new();
    options.set("rw_timeout", NETWORK_IO_TIMEOUT_US);
    options
}

/// Opens a media input, applying a read timeout for network sources so a
/// stalled remote server cannot hang the playout thread forever.
pub(crate) fn open_media_input(path: &str) -> Result<format::context::Input, FfmpegError> {
    if is_network_url(path) {
        format::input_with_dictionary(&path, network_io_options())
    } else {
        format::input(&path)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_live_input, is_network_url};

    #[test]
    fn classifies_network_urls() {
        assert!(is_network_url("rtmp://example.com/live/stream"));
        assert!(is_network_url("http://example.com/video.mp4"));
        assert!(!is_network_url("/var/lib/media/clip.mp4"));
        assert!(!is_network_url("file:///var/lib/media/clip.mp4"));
        assert!(!is_network_url("clip.mp4"));
    }

    #[test]
    fn identifies_unseekable_live_input_transports() {
        for input in [
            "rtmp://example.com/live/stream",
            "rtmps://example.com/live/stream",
            "rtsp://example.com/live",
            "rtp://239.0.0.1:5004",
            "srt://example.com:9000",
            "udp://239.0.0.1:1234",
            "tcp://example.com:9000",
            "rist://example.com:8193",
        ] {
            assert!(is_live_input(input), "{input}");
        }
        assert!(!is_live_input("https://example.com/vod.mp4"));
        assert!(!is_live_input("/var/lib/media/clip.mp4"));
    }
}
