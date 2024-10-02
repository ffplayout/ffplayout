ffplayout supports different types of outputs, let's explain them a bit:

## Stream

The streaming output can be used for any kind of classical streaming, such as **rtmp, srt, rtp**, etc. Any streaming type supported by ffmpeg should work.

**Remember that you need a streaming server as a destination if you want to use this mode.**

For example, you can use:

- [SRS](https://github.com/ossrs/srs)
- [OvenMediaEngine](https://www.ovenmediaengine.com/ome)
- [Nginx-RTMP](https://www.digitalocean.com/community/tutorials/how-to-set-up-a-video-streaming-server-using-nginx-rtmp-on-ubuntu-20-04)
- [Ant-Media-Server](https://github.com/ant-media/Ant-Media-Server)

Of course, you can also use media platforms that support streaming input.

### Multiple Outputs:

ffplayout supports multiple outputs in such a way that it can send the same stream to multiple targets with different encoding settings.

For example, if you want to stream at different resolutions, you could apply these output parameters:

```YAML
    ...

    output_param: >-
        -c:v:0 libx264
        -crf 23
        -x264-params keyint=50:min-keyint=25:scenecut=-1
        -maxrate:0 1300k
        -bufsize:0 2600k
        -preset faster
        -tune zerolatency
        -profile:v Main
        -level 3.1
        -c:a:0 aac
        -ar:0 44100
        -b:a:0 128k
        -flags +global_header
        -f flv rtmp://example.org/live/stream-high
        -s 960x540
        -c:v:1 libx264
        -crf 23
        -x264-params keyint=50:min-keyint=25:scenecut=-1
        -maxrate:1 1000k
        -bufsize:1 1800k
        -preset faster
        -tune zerolatency
        -profile:v Main
        -level 3.1
        -c:a:1 aac
        -ar:1 44100
        -b:a:1 128k
        -flags +global_header
        -f flv rtmp://example.org/live/stream-low
```

When you are using the text overlay filter, it will apply to all outputs.

The same applies to HLS output.

If you want to use different resolutions, you should apply them in order from largest to smallest. Use the largest resolution in the config under `processing:` and the smaller ones in `output_params:`.

## Desktop

In desktop mode, you will get your picture on the screen. For this, you need a desktop system; theoretically, all platforms should work here. ffplayout will require **ffplay** for that.

## HLS

In this mode, you can output directly to an HLS playlist. The nice thing here is that ffplayout requires fewer resources than in streaming mode.

HLS output is currently the default, mostly because it works out of the box and doesn't need a streaming target. By default, it saves the segments to **/usr/share/ffplayout/public/live/**.

**It is recommended to serve the HLS stream with nginx or another web server, and not with ffplayout (which is more meant for previewing).**

**HLS multiple outputs example:**

```YAML
    ...

    output_param: >-
        -filter_complex [0:v]split=3[v1_out][v2][v3];[v2]scale=w=960:h=540[v2_out];[v3]scale=w=640:h=360[v3_out];[0:a]asplit=3[a1][a2][a3]
        -map [v1_out]
        -map [a1]
        -c:v:0 libx264
        -crf 23
        -x264-params keyint=50:min-keyint=25:scenecut=-1
        -maxrate:0 2000k
        -bufsize:0 3200k
        -preset faster
        -tune zerolatency
        -profile:v Main
        -flags +cgop
        -c:a:0 aac
        -ar:0 44100
        -b:a:0 128k
        -map [v2_out]
        -map [a2]
        -c:v:1 libx264
        -crf 23
        -x264-params keyint=50:min-keyint=25:scenecut=-1
        -maxrate:1 1100k
        -bufsize:1 2200k
        -preset faster
        -tune zerolatency
        -profile:v Main
        -flags +cgop
        -c:a:1 aac
        -ar:1 44100
        -b:a:1 96k
        -map [v3_out]
        -map [a3]
        -c:v:2 libx264
        -crf 23
        -x264-params keyint=50:min-keyint=25:scenecut=-1
        -maxrate:2 800k
        -bufsize:2 1400k
        -preset faster
        -tune zerolatency
        -profile:v Main
        -flags +cgop
        -c:a:2 aac
        -ar:2 44100
        -b:a:2 64k
        -f hls
        -hls_time 6
        -hls_list_size 600
        -hls_flags append_list+delete_segments+omit_endlist
        -hls_segment_filename /var/www/html/live/stream_%v-%d.ts
        -master_pl_name master.m3u8
        -var_stream_map "v:0,a:0,name:720p v:1,a:1,name:540p v:2,a:2,name:360p"
        /var/www/html/live/stream_%v.m3u8
```

The using of **-filter_complex** and *mapping* is very limited, don't use it in situations other then for splitting the outputs.

## Tee Muxer:

The tee pseudo-muxer in FFmpeg is crucial in live streaming scenarios where a single input needs to be encoded once and then broadcast to multiple outputs in different formats or protocols. This feature significantly reduces computational overhead and improves efficiency—in my tests, it achieved a 200% reduction in CPU processing expenditure—by eliminating the need for multiple FFmpeg instances or re-encoding the same input multiple times for different outputs.

**FFmpeg's Tee Pseudo-Muxer Parameter Configuration:**

The configuration of the tee pseudo-muxer in FFmpeg allows for the broadcasting of a single input to multiple outputs simultaneously, each with specific settings. This is accomplished by specifying distinct formats and protocols for each output within a single command line, thus minimizing computational load by avoiding re-encoding for each target.

### Parameters and Syntax:

```shell
-c:v libx264
-crf 23
-x264-params keyint=50:min-keyint=25:scenecut=-1
-maxrate 1300k
-bufsize 2600k
-preset faster
-tune zerolatency
-profile:v Main
-level 3.1
-c:a aac
-ar 44100
-b:a 128k
-flags +cgop
-flags +global_header
-f tee
[f=flv:onfail=ignore]rtmp://127.0.0.1:1935/798e3a9e-47b5-4cd5-8079-76a20e03fee6.stream|[f=mpegts:onfail=ignore]udp://127.0.0.1:1234?pkt_size=1316|[f=hls:hls_time=6:hls_list_size=600:hls_flags=append_list+delete_segments+omit_endlist:hls_segment_filename=/usr/share/ffplayout/public/live/stream-%d.ts]/usr/share/ffplayout/public/live/stream.m3u8
```


**1. `-f tee`**: Specifies the use of the tee pseudo-muxer, which facilitates the multiplexing of the broadcast.

**2. Use of “|” (pipe)**: The pipe symbol "|" acts as a separator between the different outputs within the tee command. Each segment separated by a pipe configures a distinct output for the broadcast.

**3. Stream Processing by the Tee**:
   - **First Output**: `[f=flv:onfail=ignore]rtmp://127.0.0.1:1935/798e3a9e-47b5-4cd5-8079-76a20e03fee6.stream`
     - **f=flv**: Sets the output format to FLV (Flash Video).
     - **onfail=ignore**: Directs FFmpeg to continue operating even if this output fails.

   - **Second Output**: `[f=mpegts:onfail=ignore]udp://127.0.0.1:1234?pkt_size=1316`
     - **f=mpegts**: Sets the output format to MPEG-TS (MPEG Transport Stream).
     - **udp://...**: Uses the UDP protocol to send the stream with a specified packet size (`pkt_size=1316`).

   - **Third Output**: `[f=hls:hls_time=6:hls_list_size=600:hls_flags=append_list+delete_segments+omit_endlist:hls_segment_filename=/usr/share/ffplayout/public/live/stream-%d.ts]/usr/share/ffplayout/public/live/stream.m3u8`
     - **f=hls**: Sets the output format to HLS (HTTP Live Streaming).

Each stream is processed by the tee pseudo-muxer, which encodes the input only once, directing it to various outputs as specified, thereby allowing for efficient and less resource-intensive operation.
