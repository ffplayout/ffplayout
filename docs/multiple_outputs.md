### Multiple Outputs

ffplayout supports multiple outputs in a way, that it can output the same stream to multiple targets with different encoding settings.

For example you want to stream different resolutions, you could apply this output parameters:

```YAML
    ...

    output_param: >-
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
        -flags +global_header
        -f flv rtmp://example.org/live/stream-high
        -s 960x540
        -c:v libx264
        -crf 23
        -x264-params keyint=50:min-keyint=25:scenecut=-1
        -maxrate 1000k
        -bufsize 1800k
        -preset faster
        -tune zerolatency
        -profile:v Main
        -level 3.1
        -c:a aac
        -ar 44100
        -b:a 128k
        -flags +global_header
        -f flv rtmp://example.org/live/stream-low
```

When you are using the text overlay filter, it will apply to all outputs.

The same works to for HLS output.

If you want to use different resolution, you should apply them in order from biggest to smallest. Use the biggest resolution in config under `processing:` and the smaller ones in `output_params:`.
