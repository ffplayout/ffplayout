ffplayout supports different types of outputs, let's explain them a bit:

## Stream

The streaming output can be used for ever kind of classical streaming. For example for **rtmp, srt, rtp** etc. Every streaming type, which are supported from ffmpeg should be working

### Multiple Outputs:

If you would like to have multiple outputs, you can add you settings to `output_param:` like:

```yam
...
output_param: >-
    ...
    -flags +global_header
    -f flv rtmp://127.0.0.1/live/big
    -s 1280x720
    -c:v libx264
    -crf 23
    -x264-params keyint=50:min-keyint=25:scenecut=-1
    -maxrate 2400k
    -bufsize 4800k
    -preset medium
    -profile:v Main
    -level 3.1
    -c:a aac
    -ar 44100
    -b:a 128k
    -flags +global_header
    -f flv rtmp://127.0.0.1/live/middle
    -s 640x360
    -c:v libx264
    -crf 23
    -x264-params keyint=50:min-keyint=25:scenecut=-1
    -maxrate 600k
    -bufsize 1200k
    -preset medium
    -profile:v Main
    -level 3.1
    -c:a aac
    -ar 44100
    -b:a 128k
    -flags +global_header
    -f flv rtmp://127.0.0.1/live/small
```

## Desktop

In desktop mode you will get your picture on screen. For this you need a desktop system, theoretical all platforms should work here. ffplayout will need for that **ffplay**.

## HLS

In this mode you can output directly to a hls playlist. The nice thing here is, that ffplayout need less resources then in streaming mode.

#### Activating Output

To use one of the outputs you need to edit the **ffplayout.yml** config, here under **out** set your **mode** and use the different **output** options.
