use std::{collections::BTreeMap, ffi::CString, mem, ptr};

use ffmpeg_next::ffi;

use super::config::StreamType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputProtocol {
    Rtmp,
    Srt,
    Udp,
    Tcp,
    Http,
}

/// Validates AVIO options against the selected protocol's native metadata,
/// with additional restrictions only for the playout lifecycle. Child protocol
/// options are not accepted implicitly; unused options are checked on open.
pub fn validate_output_protocol_options(
    stream_type: StreamType,
    url: &str,
    options: &BTreeMap<String, String>,
) -> Result<(), String> {
    if options.is_empty() {
        return Ok(());
    }

    let scheme = url
        .split_once("://")
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .ok_or_else(|| "protocol options require a network output URL".to_string())?;
    let protocol = match stream_type {
        StreamType::Rtmp if matches!(scheme.as_str(), "rtmp" | "rtmps") => OutputProtocol::Rtmp,
        StreamType::Srt if scheme == "srt" => OutputProtocol::Srt,
        StreamType::Udp if scheme == "udp" => OutputProtocol::Udp,
        StreamType::Custom => match scheme.as_str() {
            "rtmp" | "rtmps" => OutputProtocol::Rtmp,
            "srt" => OutputProtocol::Srt,
            "udp" => OutputProtocol::Udp,
            "tcp" => OutputProtocol::Tcp,
            "http" | "https" => OutputProtocol::Http,
            _ => {
                return Err(format!(
                    "protocol options are not supported for custom {scheme:?} outputs"
                ));
            }
        },
        _ => {
            return Err(format!(
                "protocol options for {stream_type:?} output are incompatible with {scheme:?} URL"
            ));
        }
    };

    for (name, value) in options {
        let name = name.as_str();

        if name.trim() != name || name.is_empty() || value.trim().is_empty() {
            return Err("protocol option names and values must not be empty".to_string());
        }

        if matches!(name, "rw_timeout" | "timeout" | "listen_timeout") {
            return Err(format!(
                "protocol option {name:?} is managed by ffplayout and cannot be overridden"
            ));
        }
        validate_protocol_option(protocol, &scheme, name, value)?;
    }

    validate_url_option_conflicts(protocol, url, options)?;

    Ok(())
}

/// Validate options for a listening network input without opening a socket.
/// FFmpeg owns value types and bounds; ffplayout reserves lifecycle and RTMP
/// stream-key settings so the configured listener cannot be silently changed.
pub fn validate_input_protocol_options(
    scheme: &str,
    options: &BTreeMap<String, String>,
) -> Result<(), String> {
    if !matches!(scheme, "rtmp" | "srt") {
        return Err("unsupported live input protocol".to_string());
    }

    if options.len() > 32 {
        return Err("at most 32 live input options are supported".to_string());
    }

    for (name, value) in options {
        if name.is_empty()
            || name.len() > 128
            || name.trim() != name
            || value.trim().is_empty()
            || value.len() > 2048
        {
            return Err("live input option names or values are invalid".to_string());
        }

        if matches!(
            name.as_str(),
            "rw_timeout" | "timeout" | "listen_timeout" | "listen" | "rtmp_listen" | "mode"
        ) || (scheme == "rtmp" && matches!(name.as_str(), "rtmp_app" | "rtmp_playpath"))
        {
            return Err(format!(
                "live input option {name:?} is managed by ffplayout"
            ));
        }

        let number =
            validate_native_option_for(scheme, name, value, ffi::AV_OPT_FLAG_DECODING_PARAM)?;

        if scheme == "srt" {
            let valid = match name.as_str() {
                "passphrase" => (10..=64).contains(&value.len()),
                "pbkeylen" => matches!(number, Some(16 | 24 | 32)),
                "streamid" | "srt_streamid" => value.len() <= 512,
                _ => true,
            };

            if !valid {
                return Err(format!(
                    "live input option {name:?} violates SRT constraints"
                ));
            }
        }
    }

    Ok(())
}

/// Validate live demuxer settings against the linked FFmpeg build without
/// opening a network connection. `format` is an ffplayout routing key, not an
/// AVOption: it selects the input format passed to avformat_open_input.
pub fn validate_live_demuxer_options(
    backend: &str,
    options: &BTreeMap<String, String>,
) -> Result<(), String> {
    let format = match (backend, options.get("format").map(String::as_str)) {
        ("rtmp", None | Some("flv")) => "flv",
        ("rtmp", Some("live_flv")) => "live_flv",
        ("srt", None | Some("mpegts")) => "mpegts",
        ("rtmp" | "srt", Some(_)) => {
            return Err("unsupported live input demuxer format".to_string());
        }
        _ => return Err("unsupported live input backend".to_string()),
    };

    if options.len() > 32 {
        return Err("at most 32 live demuxer options are supported".to_string());
    }

    let format_name = CString::new(format).map_err(|_| "invalid input format".to_string())?;
    let input_format = unsafe { ffi::av_find_input_format(format_name.as_ptr()) };

    if input_format.is_null() {
        return Err(format!("FFmpeg input format {format:?} is unavailable"));
    }

    for (name, value) in options {
        if name == "format" {
            continue;
        }

        if name.is_empty()
            || name.len() > 128
            || name.trim() != name
            || value.trim().is_empty()
            || value.len() > 2048
        {
            return Err("live demuxer option names or values are invalid".to_string());
        }

        // Format-context options are independent of the selected demuxer.
        // Restrict them to input probing controls; other generic options can
        // alter stream selection and playout timing unexpectedly.
        let class = match name.as_str() {
            "probesize" | "analyzeduration" | "max_probe_packets" => unsafe {
                ffi::avformat_get_class()
            },
            _ => {
                if backend == "srt" && !options.contains_key("format") {
                    return Err("SRT-specific demuxer options require format=mpegts".to_string());
                }

                unsafe { (*input_format).priv_class }
            }
        };

        validate_option_class(class, name, value, ffi::AV_OPT_FLAG_DECODING_PARAM)?;
    }

    Ok(())
}

fn canonical_option_name(protocol: OutputProtocol, name: &str) -> &str {
    match (protocol, name) {
        (OutputProtocol::Srt, "payload_size") => "pkt_size",
        (OutputProtocol::Srt, "srt_streamid") => "streamid",
        (OutputProtocol::Udp, "local_port") => "localport",
        (OutputProtocol::Udp, "reuse_socket") => "reuse",
        _ => name,
    }
}

fn validate_url_option_conflicts(
    protocol: OutputProtocol,
    url: &str,
    options: &BTreeMap<String, String>,
) -> Result<(), String> {
    // HTTP and RTMP queries belong to the remote application, not AVIO options.
    if !matches!(
        protocol,
        OutputProtocol::Srt | OutputProtocol::Udp | OutputProtocol::Tcp
    ) {
        return Ok(());
    }

    let mut configured = BTreeMap::new();

    for (name, value) in options {
        let name = canonical_option_name(protocol, name);

        if configured
            .insert(name, value.as_str())
            .is_some_and(|old| old != value)
        {
            return Err(format!("conflicting aliases for protocol option {name:?}"));
        }
    }

    let Some((_, query)) = url.split_once('?') else {
        return Ok(());
    };

    for parameter in query.split('&') {
        let (name, value) = parameter.split_once('=').unwrap_or((parameter, ""));
        let name = canonical_option_name(protocol, name);
        // Match FFmpeg's av_find_info_tag: '+' means space; percent escapes
        // are not decoded. Check every occurrence to reject ambiguous duplicates.
        let value = value.replace('+', " ");

        if configured
            .get(name)
            .is_some_and(|configured| *configured != value)
        {
            // Never include URL or values: either may contain credentials.
            return Err(format!(
                "protocol option {name:?} conflicts with the output URL; configure it in only one place or use identical values"
            ));
        }
    }

    Ok(())
}

fn validate_protocol_option(
    protocol: OutputProtocol,
    scheme: &str,
    name: &str,
    value: &str,
) -> Result<(), String> {
    let number = validate_native_option(scheme, name, value)?;
    // Only playout lifecycle restrictions belong here. FFmpeg validates types,
    // bounds and constants; transport-specific semantics are checked on open.
    let valid = match (protocol, name) {
        (OutputProtocol::Srt, "mode") => matches!(number, Some(0 | 2)),
        (_, "listen" | "rtmp_listen") => number == Some(0),
        (OutputProtocol::Srt, "connect_timeout") => {
            number.is_some_and(|v| (0..=10_000).contains(&v))
        }
        (OutputProtocol::Srt, "linger") => number.is_some_and(|v| (-1..=10).contains(&v)),
        _ => true,
    };

    if !valid {
        return Err(format!(
            "protocol option {name:?} violates output lifecycle constraints"
        ));
    }

    Ok(())
}

/// Storage for the supported scalar AVOption types. This is deliberately not
/// an FFmpeg protocol context: no connection, protocol callbacks or private
/// structure layout is needed.
#[repr(C)]
struct OptionValue {
    class: *const ffi::AVClass,
    storage: [u64; 2],
}

fn validate_native_option(scheme: &str, name: &str, value: &str) -> Result<Option<i64>, String> {
    validate_native_option_for(scheme, name, value, ffi::AV_OPT_FLAG_ENCODING_PARAM)
}

fn validate_native_option_for(
    scheme: &str,
    name: &str,
    value: &str,
    direction: i32,
) -> Result<Option<i64>, String> {
    let scheme = CString::new(scheme).map_err(|_| "invalid output protocol")?;
    // SAFETY: FFmpeg returns a static protocol class for this name.
    unsafe {
        let class = ffi::avio_protocol_get_class(scheme.as_ptr());

        if class.is_null() {
            return Err("output protocol options unavailable in this FFmpeg build".to_string());
        }

        validate_option_class(class, name, value, direction)
    }
}

fn validate_option_class(
    class: *const ffi::AVClass,
    name: &str,
    value: &str,
    direction: i32,
) -> Result<Option<i64>, String> {
    let key = CString::new(name).map_err(|_| "option names must not contain null bytes")?;
    let value = CString::new(value).map_err(|_| "option values must not contain null bytes")?;

    if class.is_null() {
        return Err(format!("FFmpeg options are unavailable for {name:?}"));
    }

    // SAFETY: AVClass/AVOption definitions are static FFmpeg metadata. The
    // selected scalar option is relocated into aligned local storage below.
    unsafe {
        let fake = (&class as *const *const ffi::AVClass).cast_mut().cast();
        let option = ffi::av_opt_find(
            fake,
            key.as_ptr(),
            ptr::null(),
            direction,
            ffi::AV_OPT_SEARCH_FAKE_OBJ,
        );

        if option.is_null() {
            return Err(format!("FFmpeg does not support option {name:?}"));
        }
        use ffi::AVOptionType::*;

        if !matches!(
            (*option).type_,
            AV_OPT_TYPE_INT
                | AV_OPT_TYPE_INT64
                | AV_OPT_TYPE_BOOL
                | AV_OPT_TYPE_FLAGS
                | AV_OPT_TYPE_FLOAT
                | AV_OPT_TYPE_DOUBLE
                | AV_OPT_TYPE_STRING
        ) {
            return Err(format!("unsupported FFmpeg option type for {name:?}"));
        }
        // Relocate just the selected scalar into our own aligned storage.
        // Copy constants so FFmpeg retains symbolic values such as caller.
        let mut selected = *option;
        selected.offset = mem::offset_of!(OptionValue, storage) as i32;
        let mut options = vec![selected];
        let mut current = ptr::null();

        loop {
            current = ffi::av_opt_next(fake, current);

            if current.is_null() {
                break;
            }

            if (*current).type_ == AV_OPT_TYPE_CONST {
                options.push(*current);
            }
        }

        let mut sentinel = selected;
        sentinel.name = ptr::null();
        options.push(sentinel);
        // Do not copy callbacks: they require their original private structure.
        let validation_class = ffi::AVClass {
            class_name: c"protocol option validation".as_ptr(),
            item_name: Some(ffi::av_default_item_name),
            option: options.as_ptr(),
            version: ffi::avutil_version() as i32,
            ..mem::zeroed()
        };
        let mut storage = OptionValue {
            class: &validation_class,
            storage: [0; 2],
        };
        let object = (&mut storage as *mut OptionValue).cast();
        let result = ffi::av_opt_set(object, key.as_ptr(), value.as_ptr(), 0);
        let mut number = 0;
        let numeric = result >= 0
            && matches!(
                selected.type_,
                AV_OPT_TYPE_INT | AV_OPT_TYPE_INT64 | AV_OPT_TYPE_BOOL | AV_OPT_TYPE_FLAGS
            )
            && ffi::av_opt_get_int(object, key.as_ptr(), 0, &mut number) >= 0;
        // Frees a possible string allocation, never our stack-backed storage.
        ffi::av_opt_free(object);

        if result < 0 {
            return Err(format!(
                "invalid FFmpeg option {name:?}: {}",
                ffmpeg_next::Error::from(result)
            ));
        }

        Ok(numeric.then_some(number))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_demuxer_options_are_scoped_to_backend_and_format() {
        assert!(validate_live_demuxer_options("rtmp", &BTreeMap::new()).is_ok());
        assert!(validate_live_demuxer_options("srt", &BTreeMap::new()).is_ok());

        let live_flv = BTreeMap::from([
            ("format".to_string(), "live_flv".to_string()),
            ("flv_ignore_prevtag".to_string(), "1".to_string()),
        ]);
        assert!(validate_live_demuxer_options("rtmp", &live_flv).is_ok());
        assert!(validate_live_demuxer_options("srt", &live_flv).is_err());

        let mpegts = BTreeMap::from([
            ("format".to_string(), "mpegts".to_string()),
            ("scan_all_pmts".to_string(), "1".to_string()),
        ]);
        assert!(validate_live_demuxer_options("srt", &mpegts).is_ok());
        assert!(validate_live_demuxer_options("rtmp", &mpegts).is_err());
        assert!(
            validate_live_demuxer_options(
                "srt",
                &BTreeMap::from([("scan_all_pmts".to_string(), "1".to_string())]),
            )
            .is_err()
        );

        for options in [
            BTreeMap::from([("format".to_string(), "unknown".to_string())]),
            BTreeMap::from([("flv_ignore_prevtag".to_string(), "invalid".to_string())]),
            BTreeMap::from([("not_an_option".to_string(), "1".to_string())]),
        ] {
            assert!(validate_live_demuxer_options("rtmp", &options).is_err());
        }

        assert!(
            validate_live_demuxer_options(
                "rtmp",
                &BTreeMap::from([("probesize".to_string(), "32768".to_string())]),
            )
            .is_ok()
        );
    }

    #[test]
    fn srt_input_options_use_native_validation_and_preserve_listener_lifecycle() {
        let valid = BTreeMap::from([
            ("passphrase".to_string(), "secret-passphrase".to_string()),
            ("pbkeylen".to_string(), "16".to_string()),
            ("latency".to_string(), "120000".to_string()),
        ]);
        assert!(validate_input_protocol_options("srt", &valid).is_ok());

        for (name, value) in [
            ("passphrase", "short"),
            ("pbkeylen", "17"),
            ("latency", "not-a-number"),
            ("mode", "caller"),
            ("timeout", "0"),
            ("not_an_option", "1"),
        ] {
            let options = BTreeMap::from([(name.to_string(), value.to_string())]);
            assert!(
                validate_input_protocol_options("srt", &options).is_err(),
                "accepted {name}"
            );
        }
    }

    fn validate(scheme: &str, name: &str, value: &str) -> Result<(), String> {
        validate_output_protocol_options(
            StreamType::Custom,
            &format!("{scheme}://127.0.0.1:9000"),
            &BTreeMap::from([(name.to_string(), value.to_string())]),
        )
    }

    #[test]
    fn linked_ffmpeg_defines_numeric_types_and_bounds() {
        assert!(validate("rtmp", "tcp_nodelay", "1").is_ok());
        assert!(validate("rtmp", "tcp_nodelay", "true").is_err());
        assert!(validate("udp", "connect", "true").is_ok());
        assert!(validate("srt", "mss", "1500").is_ok());
        assert!(validate("srt", "mss", "2000").is_err());
        assert!(validate("srt", "sndbuf", "2147483648").is_err());
        assert!(validate("rtmp", "rtmp_buffer", "2147483648").is_err());
        assert!(validate("srt", "mode", "caller").is_ok());
        assert!(validate("srt", "mode", "1+0").is_err());
    }

    #[test]
    fn rejects_null_bytes_and_unmodified_invalid_names() {
        assert!(validate("rtmp", "rtmp_app", "abc\0def").is_err());
        assert!(validate("rtmp", "rtmp_app\0", "app").is_err());
        assert!(validate("udp", "pkt_size ", "1316").is_err());
        assert!(validate("udp", "pkt_size", "").is_err());
    }

    #[test]
    fn transport_string_constraints_are_checked_on_open() {
        assert!(validate("srt", "passphrase", " 12345678 ").is_ok());
        assert!(validate("srt", "passphrase", &"é".repeat(40)).is_ok());
        assert!(validate("srt", "streamid", &"é".repeat(257)).is_ok());
        assert!(validate_native_option("udp", "not_an_option", "1").is_err());
        assert!(validate_native_option("nonexistent", "pkt_size", "1316").is_err());
    }

    #[test]
    fn native_output_options_do_not_need_an_allowlist() {
        assert!(validate("http", "auth_type", "basic").is_ok());
        // Decode-only options are not output options, even without an allowlist.
        assert!(validate("http", "user_agent", "ffplayout").is_err());
        assert!(validate("udp", "not_an_option", "1").is_err());
        assert!(validate("srt", "pbkeylen", "0").is_ok());
    }

    #[test]
    fn protects_output_lifecycle() {
        for (scheme, key, value) in [
            ("tcp", "listen", "1"),
            ("http", "listen", "2"),
            ("rtmp", "rtmp_listen", "1"),
            ("srt", "mode", "listener"),
            ("srt", "mode", "1+0"),
            ("srt", "connect_timeout", "10001"),
            ("srt", "linger", "11"),
            ("tcp", "rw_timeout", "0"),
            ("srt", "timeout", "-1"),
            ("srt", "listen_timeout", "-1"),
        ] {
            assert!(validate(scheme, key, value).is_err(), "{scheme}: {key}");
        }
        assert!(validate("tcp", "listen", "0").is_ok());
        assert!(validate("srt", "mode", "rendezvous").is_ok());
    }

    #[test]
    fn rejects_conflicting_url_options_and_aliases() {
        for (scheme, query, key, value) in [
            ("udp", "pkt_size=376", "pkt_size", "188"),
            ("udp", "pkt_size=188&pkt_size=376", "pkt_size", "188"),
            ("udp", "localport=9001", "local_port", "9002"),
            ("srt", "payload_size=1316", "pkt_size", "188"),
            ("srt", "pkt_size=1316", "payload_size", "188"),
            ("srt", "mode=listener", "mode", "caller"),
            ("srt", "latency=2000000", "latency", "20000"),
            ("tcp", "tcp_nodelay=0", "tcp_nodelay", "1"),
            (
                "srt",
                "passphrase=secret-in-url",
                "passphrase",
                "secret-in-options",
            ),
        ] {
            let error = validate_output_protocol_options(
                StreamType::Custom,
                &format!("{scheme}://127.0.0.1:9000?{query}"),
                &BTreeMap::from([(key.to_string(), value.to_string())]),
            )
            .unwrap_err();
            assert!(error.contains("conflicts with the output URL"), "{error}");
            assert!(!error.contains("secret-in-"));
        }

        let options = BTreeMap::from([
            ("pkt_size".to_string(), "188".to_string()),
            ("payload_size".to_string(), "1316".to_string()),
        ]);
        assert!(
            validate_output_protocol_options(StreamType::Srt, "srt://localhost:9000", &options,)
                .unwrap_err()
                .contains("conflicting aliases")
        );
    }

    #[test]
    fn preserves_matching_values_and_application_queries() {
        for (scheme, query, key, value) in [
            ("udp", "pkt_size=188&ttl=5", "pkt_size", "188"),
            ("srt", "payload_size=1316", "pkt_size", "1316"),
            ("srt", "streamid=a+b%20c", "streamid", "a b%20c"),
            ("http", "method=remote-parameter", "method", "POST"),
            ("https", "method=remote-parameter", "method", "POST"),
            ("rtmp", "rtmp_app=remote-parameter", "rtmp_app", "live"),
        ] {
            assert!(
                validate_output_protocol_options(
                    StreamType::Custom,
                    &format!("{scheme}://127.0.0.1:9000?{query}"),
                    &BTreeMap::from([(key.to_string(), value.to_string())]),
                )
                .is_ok(),
                "{scheme}: {query}"
            );
        }
    }

    #[test]
    fn tls_options_are_unsupported_but_legacy_urls_are_unchanged() {
        assert!(
            validate("tls", "tcp_nodelay", "1")
                .unwrap_err()
                .contains("not supported for custom")
        );

        for url in [
            "tls://localhost:9000",
            "srt://localhost:9000?mode=listener&timeout=-1",
        ] {
            assert!(
                validate_output_protocol_options(StreamType::Custom, url, &BTreeMap::new(),)
                    .is_ok()
            );
        }
    }
}
