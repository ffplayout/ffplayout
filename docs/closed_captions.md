## Closed Captions

#### Note:
**FFmpeg 7.0+ development libraries are required. Builds with `libavformat` before v61.9.100 can write WebVTT HLS subtitles, but may not support custom subtitle display names in the generated master playlist. ffplayout detects this capability and omits unsupported HLS options when needed.**

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
