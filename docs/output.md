ffplayout supports different types of outputs, let's explain them a bit:

## Stream

The streaming output can be used for ever kind of classical streaming. For example for **rtmp, srt, rtp** etc. Any streaming type supported by ffmpeg should work.

**Remember that you need a streaming server as a destination if you want to use this mode.**

You can use for example:

- [SRS](https://github.com/ossrs/srs)
- [OvenMediaEngine](https://www.ovenmediaengine.com/ome)
- [Nginx-RTMP](https://www.digitalocean.com/community/tutorials/how-to-set-up-a-video-streaming-server-using-nginx-rtmp-on-ubuntu-20-04)
- [Ant-Media-Server](https://github.com/ant-media/Ant-Media-Server)

Of course, you can also use media platforms that support streaming input.

### Multiple Outputs:

ffplayout supports multiple outputs in a way, that it can output the same stream to multiple targets with different encoding settings.

For example you want to stream different resolutions, you could apply this output parameters:

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

The same works to for HLS output.

If you want to use different resolution, you should apply them in order from biggest to smallest. Use the biggest resolution in config under `processing:` and the smaller ones in `output_params:`.

## Desktop

In desktop mode you will get your picture on screen. For this you need a desktop system, theoretical all platforms should work here. ffplayout will need for that **ffplay**.

## HLS

In this mode you can output directly to a hls playlist. The nice thing here is, that ffplayout need less resources then in streaming mode.

HLS output is currently the default, mostly because it works out of the box and don't need a streaming target. In default settings it saves the segments to **/usr/share/ffplayout/public/live/**.

**It is recommend to serve the HLS stream with nginx or another web server, and not with ffplayout (which is more meant for previewing).**

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

#### Activating Output

To use one of the outputs you need to edit the **ffplayout.yml** config, here under **out** set your **mode** and use the different **output** options.
