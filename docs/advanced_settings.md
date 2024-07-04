## Advanced settings

With **advanced settings** you can control all ffmpeg inputs/decoder output and filters.

> **_Note:_** Changing these settings is for advanced users only! There will be no support or guarantee that it will work and be stable after changing them!

For changing this settings you need to have knowledge about hardware encoding with ffmpeg. Good starting points are:

- [HWAccelIntro](https://trac.ffmpeg.org/wiki/HWAccelIntro)
- [VAAPI](https://trac.ffmpeg.org/wiki/Hardware/VAAPI)
- [QuickSync](https://trac.ffmpeg.org/wiki/Hardware/QuickSync)

### Example config

##### Here an example with Intel QuickSync:

```YAML
help: Changing these settings is for advanced users only! There will be no support or guarantee that ffplayout will be stable after changing them.
decoder:
    input_param: -hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv
    # output_param get also applied to ingest instance.
    output_param: -c:v mpeg2_qsv -g 1 -b:v 50000k -minrate 50000k -maxrate 50000k -bufsize 25000k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2
    filters:
        deinterlace: deinterlace_qsv
        pad_scale_w: scale_qsv={}:-1
        pad_scale_h: scale_qsv=-1:{}
        pad_video: 'null' # pad=max(iw\\,ih*({0}/{1})):ow/({0}/{1}):(ow-iw)/2:(oh-ih)/2
        fps: vpp_qsv=framerate=25
        scale: scale_qsv={}:{}
        set_dar: 'null' # setdar=dar={}
        fade_in: 'null' # fade=in:st=0:d=0.5
        fade_out: 'null' # fade=out:st={}:d=1.0
        overlay_logo_scale: 'null'
        overlay_logo_fade_in: fade=in:st=0:d=1.0 # fade=in:st=0:d=1.0:alpha=1
        overlay_logo_fade_out: fade=out:st={}:d=1.0 # fade=out:st={}:d=1.0:alpha=1
        overlay_logo: hwupload=extra_hw_frames=64,format=qsv[l];[v][l]overlay_qsv={}:shortest=1
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

##### Here an example with Nvidia HW processing

```YAML
help: Changing these settings is for advanced users only! There will be no support or guarantee that it will be stable after changing them.
decoder:
    input_param: -thread_queue_size 1024 -hwaccel_device 0 -hwaccel cuvid -hwaccel_output_format cuda
    # output_param get also applied to ingest instance.
    output_param: -c:v h264_nvenc -preset p2 -tune ll -b:v 50000k -minrate 50000k -maxrate 50000k -bufsize 25000k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2
    filters:
        deinterlace: 'null'
        pad_scale_w: 'null' # scale={}:-1
        pad_scale_h: 'null' # scale=-1:{}
        pad_video: 'null' # pad=max(iw\\,ih*({0}/{1})):ow/({0}/{1}):(ow-iw)/2:(oh-ih)/2
        fps: 'null' # fps={}
        scale: scale_cuda={}:{}:interp_algo=lanczos:force_original_aspect_ratio=decrease # scale={}:{}
        set_dar: 'null' # setdar=dar={}
        fade_in: hwdownload,format=nv12,fade=in:st=0:d=0.5,format=nv12,hwupload_cuda # fade=in:st=0:d=0.5
        fade_out: hwdownload,format=nv12,fade=out:st={}:d=1.0,format=nv12,hwupload_cuda # fade=out:st={}:d=1.0
        overlay_logo_scale: 'null' # scale={}
        overlay_logo_fade_in: fade=in:st=0:d=1.0 # fade=in:st=0:d=1.0:alpha=1
        overlay_logo_fade_out: fade=out:st={}:d=1.0 # fade=out:st={}:d=1.0:alpha=1
        overlay_logo: format=nv12,hwupload_cuda[l];[v][l]overlay_cuda=W-w-12:12:shortest=1,hwdownload,format=nv12
        tpad: # tpad=stop_mode=add:stop_duration={}
        drawtext_from_file: # drawtext=text='{}':{}{}
        drawtext_from_zmq: # zmq=b=tcp\\\\://'{}',drawtext@dyntext={}
        aevalsrc: # aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000
        afade_in: # afade=in:st=0:d=0.5
        afade_out: # afade=out:st={}:d=1.0
        apad: # apad=whole_dur={}
        volume: # volume={}
        split: # split={}{}
encoder:
    input_param:
ingest:
    input_param: -thread_queue_size 1024 -hwaccel_device 0 -hwaccel cuvid -hwaccel_output_format cuda
```

---

**At the moment this function is _experimental_, if you think you found a bug: check full decoder/encoder/ingest command with ffmpeg in terminal. When there the command works you can open a bug report issue.**

Please don't open issues for general command line helps!
