use std::{collections::BTreeMap, ffi::CString, ptr};

use ffmpeg_next::ffi;
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
    network_output_options(&BTreeMap::new())
}

/// Builds an AVIO option dictionary while retaining ownership of operational
/// timeouts. Validation happens before this function is called; setting the
/// timeout last also provides defence in depth against accidental overrides.
pub(crate) fn network_output_options(
    protocol_options: &BTreeMap<String, String>,
) -> Dictionary<'static> {
    let mut options = Dictionary::new();
    for (key, value) in protocol_options {
        options.set(key, value);
    }
    options.set("rw_timeout", NETWORK_IO_TIMEOUT_US);
    options
}

/// Opens AVIO separately so unused user options are checked and every failure
/// frees both the output context and any opened connection.
pub(crate) fn open_network_output(
    path: &str,
    muxer: Option<&str>,
    options: &BTreeMap<String, String>,
) -> anyhow::Result<format::context::Output> {
    let path = CString::new(path)?;
    let muxer = muxer.map(CString::new).transpose()?;
    // Defence in depth for callers bypassing configuration validation.
    for (key, value) in options {
        CString::new(key.as_str())?;
        CString::new(value.as_str())?;
    }
    unsafe {
        let mut context = ptr::null_mut();
        let result = ffi::avformat_alloc_output_context2(
            &mut context,
            ptr::null_mut(),
            muxer.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
            path.as_ptr(),
        );
        if result < 0 || context.is_null() {
            if !context.is_null() {
                ffi::avformat_free_context(context);
            }
            anyhow::bail!("failed to allocate network output context");
        }
        // From this point the RAII wrapper closes pb and frees the context,
        // including on an avio_open2 failure or unused-option error.
        let output = format::context::Output::wrap(context);
        let mut dictionary = network_output_options(options).disown();
        let result = ffi::avio_open2(
            &mut (*context).pb,
            path.as_ptr(),
            ffi::AVIO_FLAG_WRITE,
            ptr::null(),
            &mut dictionary,
        );
        let remaining = Dictionary::own(dictionary);
        if result < 0 {
            return Err(FfmpegError::from(result).into());
        }
        let unused: Vec<_> = remaining
            .iter()
            .filter(|(key, _)| options.contains_key(*key))
            .map(|(key, _)| key)
            .collect();
        if !unused.is_empty() {
            anyhow::bail!("unused output protocol options: {}", unused.join(", "));
        }
        Ok(output)
    }
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
    use std::collections::BTreeMap;

    use super::{is_live_input, is_network_url, network_output_options};

    #[test]
    fn udp_open_consumes_packet_size_and_rejects_unused_options() {
        let receiver = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let url = format!("udp://{}", receiver.local_addr().unwrap());
        let options = BTreeMap::from([("pkt_size".to_string(), "188".to_string())]);
        let mut output = super::open_network_output(&url, Some("mpegts"), &options).unwrap();
        unsafe {
            let pb = (*output.as_mut_ptr()).pb;
            assert_eq!((*pb).max_packet_size, 188);
            let data = [0x47u8; 376];
            ffmpeg_next::ffi::avio_write(pb, data.as_ptr(), data.len() as i32);
            ffmpeg_next::ffi::avio_flush(pb);
        }
        let mut packet = [0u8; 512];
        assert_eq!(receiver.recv(&mut packet).unwrap(), 188);
        assert_eq!(receiver.recv(&mut packet).unwrap(), 188);
        for key in ["pkt_size ", "not_an_option"] {
            let options = BTreeMap::from([(key.to_string(), "188".to_string())]);
            let error = super::open_network_output(&url, Some("mpegts"), &options)
                .err()
                .expect("unused option must fail");
            assert!(error.to_string().contains("unused output protocol options"));
        }
    }

    #[test]
    fn srt_open_consumes_latency_and_delivers_data() {
        use ffmpeg_next::ffi;
        use std::{ffi::CString, ptr, thread};
        // Reserve an ephemeral loopback port until the listener thread starts.
        let reservation = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        let listener = thread::spawn(move || {
            let url = CString::new(format!("srt://{address}?mode=listener")).unwrap();
            let mut options = super::network_io_options();
            options.set("listen_timeout", "3000000");
            let mut options = unsafe { options.disown() };
            let mut pb = ptr::null_mut();
            drop(reservation);
            unsafe {
                let result = ffi::avio_open2(
                    &mut pb,
                    url.as_ptr(),
                    ffi::AVIO_FLAG_READ,
                    ptr::null(),
                    &mut options,
                );
                drop(ffmpeg_next::Dictionary::own(options));
                if result < 0 {
                    return Err(ffmpeg_next::Error::from(result));
                }
                let mut data = [0u8; 188];
                let result = ffi::avio_read(pb, data.as_mut_ptr(), data.len() as i32);
                ffi::avio_closep(&mut pb);
                if result < 0 {
                    return Err(ffmpeg_next::Error::from(result));
                }
                Ok((result, data))
            }
        });
        let options = BTreeMap::from([
            ("latency".to_string(), "20000".to_string()),
            ("connect_timeout".to_string(), "2000".to_string()),
        ]);
        let output =
            super::open_network_output(&format!("srt://{address}"), Some("mpegts"), &options);
        if let Ok(mut output) = output {
            unsafe {
                let data = [0x47u8; 188];
                let pb = (*output.as_mut_ptr()).pb;
                ffi::avio_write(pb, data.as_ptr(), data.len() as i32);
                ffi::avio_flush(pb);
            }
            // Keep the connection alive until the peer has received the data.
            assert_eq!(listener.join().unwrap().unwrap(), (188, [0x47u8; 188]));
        } else {
            let _ = listener.join();
            panic!("SRT output failed to open: {:?}", output.err());
        }
    }

    #[test]
    fn output_options_merge_user_values_with_enforced_timeout() {
        let options = network_output_options(&BTreeMap::from([
            ("latency".to_string(), "2000000".to_string()),
            ("rw_timeout".to_string(), "0".to_string()),
        ]));

        assert_eq!(options.get("latency"), Some("2000000"));
        assert_eq!(options.get("rw_timeout"), Some("10000000"));
    }

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
