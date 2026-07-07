## Closed Captions

#### Note:
**FFmpeg 7.0+ development libraries are required. FFmpeg builds before 7.2 can write WebVTT HLS subtitles, but may not support custom subtitle display names in the generated master playlist. ffplayout detects this capability and omits unsupported HLS options when needed.**

### Usage
**ffplayout** can handle closed captions in WebVTT format for HLS streaming.

The captions are read from a separate `*.vtt` sidecar file that shares the same filename as the video file. If no sidecar file is present, ffplayout can use the configured **vtt_dummy** file as a fallback. The processing option **vtt_enable** must be enabled.

To output WebVTT subtitles, the **HLS** output mode must be enabled.
