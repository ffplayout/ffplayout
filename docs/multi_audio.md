## Multiple Audio Tracks

**\* This is an experimental feature and more intended for advanced users. Use it with caution!**

With _ffplayout_, you can output streams with multiple audio tracks, with some limitations:
* Not all formats support multiple audio tracks. For example, _flv/rtmp_ doesn't support it.
* In your output parameters, you need to set the correct mapping.

ffmpeg filter usage and encoding parameters can become very complex, so it may happen that not every combination works out of the box.

To get a better idea of what works, you can examine [engine_cmd](../tests/src/engine_cmd.rs).

If you are outputting a single video stream with multiple audio tracks, for example with the `srt://` protocol, you only need to set the correct `audio_tracks:` count in your config under `processing:`.

For multiple video resolutions and multiple audio tracks, the parameters could look like:

```YAML
out:
    ...
    mode: stream
    output_param: >-
        -map 0:v
        -map 0:a:0
        -map 0:a:1
        -c:v libx264
        -c:a aac
        -ar 44100
        -b:a 128k
        -flags +global_header
        -f mpegts
        srt://127.0.0.1:40051
        -map 0:v
        -map 0:a:0
        -map 0:a:1
        -s 512x288
        -c:v libx264
        -c:a aac
        -ar 44100
        -b:a 128k
        -flags +global_header
        -f mpegts
        srt://127.0.0.1:40052
```

If you need HLS output with multiple resolutions and audio tracks, you can try something like:

```YAML
out:
    ...
    mode: hls
    output_param: >-
        -filter_complex [0:v]split=2[v1_out][v2];[v2]scale=w=512:h=288[v2_out];[0:a:0]asplit=2[a_0_1][a_0_2];[0:a:1]asplit=2[a_1_1][a_1_2]
        -map [v1_out]
        -map [a_0_1]
        -map [a_1_1]
        -c:v libx264
        -flags +cgop
        -c:a aac
        -map [v2_out]
        -map [a_0_2]
        -map [a_1_2]
        -c:v:1 libx264
        -flags +cgop
        -c:a:1 aac
        -f hls
        -hls_time 6
        -hls_list_size 600
        -hls_flags append_list+delete_segments+omit_endlist
        -hls_segment_filename /usr/share/ffplayout/public/live/stream_%v-%d.ts
        -master_pl_name master.m3u8
        -var_stream_map "v:0,a:0,a:1,name:720p v:1,a:2,a:3,name:288p"
        /usr/share/ffplayout/public/live/stream_%v.m3u8
```
