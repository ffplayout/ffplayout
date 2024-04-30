**ffplayout**
================

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

## **ffplayout-engine (ffplayout)**

[ffplayout](/ffplayout-engine/README.md) is a 24/7 broadcasting solution. It can playout a folder containing audio or video clips, or play a *JSON* playlist for each day, keeping the current playlist editable.

The ffplayout applications are mostly designed to run as system services on Linux. But in general they should run on any platform supported by Rust.

Check the [releases](https://github.com/ffplayout/ffplayout/releases/latest) for pre compiled version.

### Features

- have all values in a separate config file
- dynamic playlist
- replace missing playlist or clip with single filler or multiple fillers from folder, if no filler exists, create dummy clip
- playing clips in [watched](/docs/folder_mode.md) folder mode
- send emails with error message
- overlay a logo
- overlay text, controllable through [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend) (needs ffmpeg with libzmq and enabled JSON RPC server)
- loop playlist infinitely
- [remote source](/docs/remote_source.md)
- trim and fade the last clip, to get full 24 hours
- when playlist is not 24 hours long, loop fillers until time is full
- set custom day start, so you can have playlist for example: from 6am to 6am, instate of 0am to 12pm
- normal system requirements and no special tools
- no GPU power is needed
- stream to server or play on desktop
- log to files or color output to console
- add filters to input, if is necessary to match output stream:
  - **yadif** (deinterlacing)
  - **pad** (letterbox or pillarbox to fit aspect)
  - **fps** (change fps)
  - **scale** (fit target resolution)
  - **aevalsrc** (if video have no audio)
  - **apad** (add silence if audio duration is to short)
  - **tpad** (add black frames if video duration is to short)
- [output](/docs/output.md):
  - **stream**
  - **desktop**
  - **HLS**
  - **null** (for debugging)
- JSON RPC server, to get information about what is playing and to control it
- [live ingest](/docs/live_ingest.md)
- image source (will loop until out duration is reached)
- extra audio source, has priority over audio from video (experimental *)
- [multiple audio tracks](/docs/multi_audio.md) (experimental *)
- [Stream Copy](/docs/stream_copy.md) mode (experimental *)
- [custom filters](/docs/custom_filters.md) globally in config, or in playlist for specific clips
- import playlist from text or m3u file, with CLI or frontend
- audio only, for radio mode (experimental *)
- [Piggyback Mode](/ffplayout-api/README.md#piggyback-mode), mostly for non Linux systems (experimental *)
- generate playlist based on [template](/docs/playlist_gen.md) (experimental *)
- During playlist import, all video clips are validated and, if desired, checked to ensure that the audio track is not completely muted.

For preview stream, read: [/docs/preview_stream.md](/docs/preview_stream.md)

**\* Experimental features do not guarantee the same stability and may fail under unusual circumstances. Code and configuration options may change in the future.**

## **ffplayout-api (ffpapi)**

ffpapi serves the [frontend](https://github.com/ffplayout/ffplayout-frontend) and it acts as a [REST API](/ffplayout-api/README.md) for controlling the engine, manipulate playlists, add settings etc.

### Requirements

- RAM and CPU depends on video resolution, minimum 4 threads and 3GB RAM for 720p are recommend
- **ffmpeg** v5.0+ and **ffprobe** (**ffplay** if you want to play on desktop)
- if you want to overlay text, ffmpeg needs to have **libzmq**

### Install

Check [install](docs/install.md) for details about how to install ffplayout.

-----

### JSON Playlist Example

```json
{
    "channel": "Test 1",
    "date": "2019-03-05",
    "program": [{
            "in": 0,
            "out": 647.68,
            "duration": 647.68,
            "source": "/Media/clip1.mp4"
        }, {
            "in": 0,
            "out": 890.02,
            "duration": 890.02,
            "source": "/Media/clip2.mp4",
            "custom_filter": "eq=gamma_b=0.6:gamma_g=0.7[c_v_out]"
        }, {
            "in": 0,
            "out": 149,
            "duration": 149,
            "source": "/Media/clip3.mp4",
            "category": "advertisement"
        }, {
            "in": 0,
            "out": 114.72,
            "duration": 114.72,
            "source": "/Media/image1.jpg",
        }, {
            "in": 0,
            "out": 230.30,
            "duration": 230.30,
            "source": "/Media/image2.jpg",
            "audio": "/Media/audio1.mp3"
        }, {
            "in": 0,
            "out": 2531.36,
            "duration": 2531.36,
            "source": "https://example.org/big_buck_bunny.webm",
            "category": ""
        }
    ]
}
```

## **Warning**

(Endless) streaming over multiple days will only work if config has a **day_start** value and the **length** value is **24 hours**. If you only need a few hours for each day, use a *cron* job or something similar.

-----

## HLS output

For outputting to HLS, output parameters should look like:

```yaml
out:
    ...

    output_param: >-
        ...

        -flags +cgop
        -f hls
        -hls_time 6
        -hls_list_size 600
        -hls_flags append_list+delete_segments+omit_endlist+program_date_time
        -hls_segment_filename /var/www/html/live/stream-%09d.ts /var/www/html/live/stream.m3u8
```

-----

## JSON RPC

The ffplayout engine can run a simple RPC server. A request looks like:

```Bash
curl -X POST -H "Content-Type: application/json" -H "Authorization: ---auth-key---" \
    -d '{"control":"next"}' \
    127.0.0.1:7070
```

At the moment this commends are possible:

```Bash
'{"media":"current"}'  # get infos about current clip
'{"media":"next"}'  # get infos about next clip
'{"media":"last"}'  # get infos about last clip
'{"control":"next"}'   # jump to next clip
'{"control":"back"}'   # jump to last clip
'{"control":"reset"}'  # reset playlist to old state
'{"control":"text", \
  "message": {"text": "Hello from ffplayout", "x": "(w-text_w)/2", "y": "(h-text_h)/2", \
  "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, \
  "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0}}' # send text to drawtext filter from ffmpeg
```

Output from `{"media":"current"}` show:

```JSON
{
    "media": {
        "category": "",
        "duration": 154.2,
        "out": 154.2,
        "in": 0.0,
        "source": "/opt/tv-media/clip.mp4"
    },
    "index": 39,
    "mode": "playlist",
    "ingest": false,
    "played": 67.80771999300123,
}
```

If you are in playlist mode and move backwards or forwards in time, the time shift is saved so the playlist is still in sync. Bear in mind, however, that this may make your playlist too short. If you do not reset it, it will automatically reset the next day.
