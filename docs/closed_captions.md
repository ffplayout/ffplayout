## Closed Captions

#### Note:
**This is only an _experimental feature_. Please be aware that bugs and unexpected behavior may occur. To utilize this feature, a version after 7.1 of FFmpeg is required. Importantly, there is currently no official support for this functionality.**

### Usage
**ffplayout** can handle closed captions in WebVTT format for HLS streaming.

The captions can be embedded in the file, such as in a [Matroska](https://www.matroska.org/technical/subtitles.html) file, or they can be a separate *.vtt file that shares the same filename as the video file. In either case, the processing option **vtt_enable** must be enabled, and the path to the **vtt_dummy** file must exist.

To encode the closed captions, the **hls** mode needs to be enabled, and specific output parameters must be provided. Here’s an example:

```
-c:v libx264 -crf 23 -x264-params keyint=50:min-keyint=25:scenecut=-1 \
-maxrate 1300k -bufsize 2600k -preset faster -tune zerolatency \
-profile:v Main -level 3.1 -c:a aac -ar 44100 -b:a 128k -flags +cgop \
-muxpreload 0 -muxdelay 0 -f hls -hls_time 6 -hls_list_size 600 \
-hls_flags append_list+delete_segments+omit_endlist \
-var_stream_map v:0,a:0,s:0,sgroup:subs,sname:English,language:en-US,default:YES \
-master_pl_name master.m3u8 \
-hls_segment_filename \
live/stream-%d.ts live/stream.m3u8
```
