## Advanced settings

Within **/etc/ffplayout/advanced.yml** you can control all ffmpeg inputs/decoder output and filters.

> **_Note:_** Changing these settings is for advanced users only! There will be no support or guarantee that it will work and be stable after changing them!

For changing this settings you need to have knowledge about hardware encoding with ffmpeg. Good starting points are:

- [HWAccelIntro](https://trac.ffmpeg.org/wiki/HWAccelIntro)
- [VAAPI](https://trac.ffmpeg.org/wiki/Hardware/VAAPI)
- [QuickSync](https://trac.ffmpeg.org/wiki/Hardware/QuickSync)

### Example config

Here an example with Intel QuickSync:

```YAML
help: Changing these settings is for advanced users only! There will be no support or guarantee that ffplayout will be stable after changing them.
decoder:
    input_param: -hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv
    # output_param get also applied to ingest instance.
    output_param: -c:v mpeg2_qsv -g 1 -b:v 50000k -minrate 50000k -maxrate 50000k -bufsize 25000k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2
    filters:
        deinterlace: deinterlace_qsv
        pad_scale_w: scale_qsv={}:-1,
        pad_scale_h: scale_qsv=-1:{},
        pad_video: 'null' # 'pad=max(iw\\,ih*({0}/{1})):ow/({0}/{1}):(ow-iw)/2:(oh-ih)/2'
        fps: vpp_qsv=framerate=25
        scale: scale_qsv={}:{}
        set_dar: 'null' # setdar=dar={}
        fade_in: 'null' # fade=in:st=0:d=0.5
        fade_out: 'null' # fade=out:st={}:d=1.0
        overlay_logo_scale: 'scale_qsv={}'
        overlay_logo: null[v];movie={}:loop=0,setpts=N/(FRAME_RATE*TB),format=rgba,colorchannelmixer=aa={}{},hwupload=extra_hw_frames=64,format=qsv[l];[v][l]overlay_qsv={}:shortest=1
        overlay_logo_fade_in: 'null' # ',fade=in:st=0:d=1.0:alpha=1'
        overlay_logo_fade_out: 'null' # ',fade=out:st={}:d=1.0:alpha=1'
        tpad: 'null' # tpad=stop_mode=add:stop_duration={}
        drawtext_from_file: hwdownload,format=nv12,drawtext=text='{}':{}{} # drawtext=text='{}':{}{}
        drawtext_from_zmq: hwdownload,format=nv12,zmq=b=tcp\\://'{}',drawtext@dyntext={} # zmq=b=tcp\\\\://'{}',drawtext@dyntext={}
        aevalsrc: # aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000
        afade_in: # afade=in:st=0:d=0.5
        afade_out: # afade=out:st={}:d=1.0
        apad: # apad=whole_dur={}
        volume: # volume={}
        split: # split={}{}
encoder:
    # use `-hwaccel vulkan` when output mode is desktop
    input_param: -hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv
ingest:
    input_param: -hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv
```

---

**At the moment this function is _experimental_, if you think you found a bug: check full decoder/encoder/ingest command with ffmpeg in terminal. When there the command works you can open a bug report issue.**

Please don't open issues for general command line helps!
