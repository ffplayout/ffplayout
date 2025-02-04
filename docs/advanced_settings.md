## Advanced settings

With **advanced settings** you can control all ffmpeg inputs/decoder/output and filters.

> **_Note:_** Changing these settings is for advanced users only! There will be no support or guarantee that it will work and be stable after changing them!

For changing this settings you need to have knowledge about hardware encoding with ffmpeg. Good starting points are:

- [HWAccelIntro](https://trac.ffmpeg.org/wiki/HWAccelIntro)
- [VAAPI](https://trac.ffmpeg.org/wiki/Hardware/VAAPI)
- [QuickSync](https://trac.ffmpeg.org/wiki/Hardware/QuickSync)

### Example config

##### Here an example with Intel QuickSync:

```TOML
[decoder]
input_param = "-hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv"
output_param = "-c:v mpeg2_qsv -g 1 -b:v 50000k -minrate 50000k -maxrate 50000k -bufsize 25000k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2" # get also applied to ingest instance.

[encoder]
input_param = ""

[filter]
deinterlace = "deinterlace_qsv" # yadif=0:-1:0
pad_video = "" # pad='ih*{}/{}:ih:(ow-iw)/2:(oh-ih)/2'
fps = "vpp_qsv=framerate=25" # fps={}
scale = "scale_qsv={}:{}" # scale={}:{}
set_dar = "" # setdar=dar={}
fade_in = "" # fade=in:st=0:d=0.5
fade_out = "" # fade=out:st={}:d=1.0
logo = ""
overlay_logo_scale = "scale_qsv={}" # scale={}
overlay_logo_fade_in = "" # fade=in:st=0:d=1.0:alpha=1
overlay_logo_fade_out = "" # fade=out:st={}:d=1.0:alpha=1
overlay_logo = "overlay_qsv={}:shortest=1" # overlay={}:shortest=1
tpad = "" # tpad=stop_mode=add:stop_duration={}
drawtext_from_file = "" # drawtext=text='{}':{}{}
drawtext_from_zmq = "" # zmq=b=tcp\\://'{}',drawtext@dyntext={}
aevalsrc = "" # aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000
afade_in = "" # afade=in:st=0:d=0.5
afade_out = "" # afade=out:st={}:d=1.0
apad = "" # apad=whole_dur={}
volume = "" # volume={}
split = "" # split={}{}

[ingest]
input_param = "-hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv"
```

##### Here an example with Nvidia HW processing

```TOML
[decoder]
input_param = "-thread_queue_size 1024 -hwaccel_device 0 -hwaccel cuvid -hwaccel_output_format cuda"
output_param = "-c:v h264_nvenc -preset p2 -tune ll -b:v 50000k -minrate 50000k -maxrate 50000k -bufsize 25000k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2" # get also applied to ingest instance.

[encoder]
input_param = ""

[filter]
deinterlace = "yadif_cuda=0:-1:0" # yadif=0:-1:0
pad_video = "" # pad='ih*{}/{}:ih:(ow-iw)/2:(oh-ih)/2'
fps = "" # fps={}
scale = "scale_cuda={}:{}:format=yuv420p" # scale={}:{}
set_dar = "" # setdar=dar={}
fade_in = "" # fade=in:st=0:d=0.5
fade_out = "" # fade=out:st={}:d=1.0
logo = ""
overlay_logo_scale = "scale_cuda={}" # scale={}
overlay_logo_fade_in = "" # fade=in:st=0:d=1.0:alpha=1
overlay_logo_fade_out = "" # fade=out:st={}:d=1.0:alpha=1
overlay_logo = "overlay_cuda={}:shortest=1" # overlay={}:shortest=1
tpad = "" # tpad=stop_mode=add:stop_duration={}
drawtext_from_file = "" # drawtext=text='{}':{}{}
drawtext_from_zmq = "" # zmq=b=tcp\\://'{}',drawtext@dyntext={}
aevalsrc = "" # aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000
afade_in = "" # afade=in:st=0:d=0.5
afade_out = "" # afade=out:st={}:d=1.0
apad = "" # apad=whole_dur={}
volume = "" # volume={}
split = "" # split={}{}

[ingest]
input_param = "-thread_queue_size 1024 -hwaccel_device 0 -hwaccel cuvid -hwaccel_output_format cuda"
```

---

**At the moment this function is _experimental_, if you think you found a bug: check full decoder/encoder/ingest command with ffmpeg in terminal. When there the command works you can open a bug report issue.**

Please don't open issues for general command line helps!
