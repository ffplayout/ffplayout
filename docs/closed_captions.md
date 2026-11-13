## Closed Captions

#### Note:
FFmpeg 7.0+ development libraries are required with either HLS packager, as the
engine uses FFmpeg for encoding and MPEG-TS muxing.

With the **FFmpeg packager**, builds with `libavformat` before v61.9.100 can write
WebVTT HLS subtitles, but may not support custom subtitle display names in the
generated master playlist. ffplayout detects this capability and omits unsupported
HLS options when needed. The **native packager** writes the master playlist itself
and supports custom subtitle display names independently of the `libavformat`
version.

### Live subtitle compatibility

The FFmpeg HLS packager currently does not write `EXT-X-PROGRAM-DATE-TIME` to
the subtitle playlist, even when `hls_flags=program_date_time` enables it in the
video playlists. Without a common date/time mapping, live subtitles can disappear
after playback has been running for a while in hls.js and in Video.js 10 players
using hls.js, although VTT segments continue to load. This was reproduced with
hls.js 1.7.3 and 1.7.4.

With the **native packager** and `hls_flags=program_date_time`, matching
`EXT-X-PROGRAM-DATE-TIME` timestamps are written to both video and subtitle
playlists. This gives hls.js a consistent date/time mapping and avoids the
reproduced playlist-alignment issue. Selecting the native packager in the web
interface adds this option by default when no explicit `hls_flags` value exists.
Existing native configurations are not changed. To disable the timestamps,
remove `hls_flags` from the output's advanced muxer settings. API clients must
set the option explicitly.

### Usage
**ffplayout** can handle closed captions in WebVTT format for HLS streaming.

The captions are read from a separate `*.vtt` sidecar file that shares the same filename as the video file. If no sidecar file is present, ffplayout can use the configured **vtt_dummy** file as a fallback. The processing option **vtt_enable** must be enabled.

Malformed subtitle blocks are skipped individually, with a warning containing the
sidecar path and the first affected block's line number. Valid cues in the same
file remain available. An unreadable sidecar is ignored for that clip; subtitle
input errors do not disconnect the HLS audio/video output.

Cues are intersected with the played source interval: seeking into a cue clips its
start, and its end is limited to the earlier of the video's declared duration and
the configured out-point. Cues entirely outside that interval are omitted. The
same interval is retained across output reconnects and applies to desktop captions.
If the source duration is unknown, an explicit out-point still provides a limit;
without either boundary, no end can be inferred in advance.

To output WebVTT subtitles, the **HLS** output mode must be enabled.
