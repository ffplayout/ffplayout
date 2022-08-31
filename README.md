**ffplayout**
================

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

## **ffplayout-engine (ffplayout)**

[ffplayout](/ffplayout-engine/README.md) is a 24/7 broadcasting solution. It can playout a folder with containing video clips, or play for every day a *JSON* playlist, while keeping the current playlist editable.

The ffplayout apps are mostly made to run on Linux as system services. But in general they should run on all platforms which are supported by Rust.

Check the [releases](https://github.com/ffplayout/ffplayout/releases/latest) for pre compiled version.

### Features

- have all values in a separate config file
- dynamic playlist
- replace missing playlist or clip with a dummy clip
- playing clips in [watched](/docs/folder_mode.md) folder mode
- send emails with error message
- overlay a logo
- overlay text, controllable through [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend) (needs ffmpeg with libzmq and enabled JSON RPC server)
- EBU R128 loudness normalization (single pass)
- loop playlist infinitely
- [remote source](/docs/remote_source.md)
- trim and fade the last clip, to get full 24 hours
- when playlist is not 24 hours long, loop filler clip until time is full
- set custom day start, so you can have playlist for example: from 6am to 6am, instate of 0am to 12pm
- normal system requirements and no special tools
- no GPU power is needed
- stream to server or play on desktop
- logging to files, or colored output to console
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
- JSON RPC server, for getting infos about current playing and controlling
- [live ingest](/docs/live_ingest.md)
- image source (will loop until out duration is reached)
- extra audio source (experimental) (has priority over audio from video source)
- [custom filter](/docs/custom_filters.md)

For preview stream, read: [/docs/preview_stream.md](/docs/preview_stream.md)

## **ffplayout-api (ffpapi)**

ffpapi serves the [frontend](https://github.com/ffplayout/ffplayout-frontend) and it acts as a [REST API](/ffplayout-api/README.md) for controlling the engine, manipulate playlists, add settings etc.

### Requirements

- RAM and CPU depends on video resolution, minimum 4 threads and 3GB RAM for 720p are recommend
- **ffmpeg** v4.2+ and **ffprobe** (**ffplay** if you want to play on desktop)
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
            "out": 149,
            "duration": 149,
            "source": "/Media/clip2.mp4",
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

(Endless) streaming over multiple days will only work when config have **day_start** value and the **length** value is **24 hours**. If you need only some hours for every day, use a *cron* job, or something similar.

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

The ffplayout engine can run a JSON RPC server. A request show look like:

```Bash
curl -X POST -H "Content-Type: application/json" -H "Authorization: ---auth-key---" \
    -d '{"jsonrpc": "2.0", "id":1, "method": "player", "params":{"control":"next"}}' \
    127.0.0.1:7070
```

At the moment this comments are possible:

```Bash
'{"jsonrpc": "2.0", "id":1, "method": "player", "params":{"media":"current"}}'  # get infos about current clip
'{"jsonrpc": "2.0", "id":2, "method": "player", "params":{"media":"next"}}'  # get infos about next clip
'{"jsonrpc": "2.0", "id":3, "method": "player", "params":{"media":"last"}}'  # get infos about last clip
'{"jsonrpc": "2.0", "id":4, "method": "player", "params":{"control":"next"}}'   # jump to next clip
'{"jsonrpc": "2.0", "id":5, "method": "player", "params":{"control":"back"}}'   # jump to last clip
'{"jsonrpc": "2.0", "id":6, "method": "player", "params":{"control":"reset"}}'  # reset playlist to old state

'{"jsonrpc": "2.0", "id":7, "method": "player", "params":{"control":"text", \
  "message": {"text": "Hello from ffplayout", "x": "(w-text_w)/2", "y": "(h-text_h)/2", \
  "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, \
  "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0}}}' # send text to drawtext filter from ffmpeg
```

Output from `{"media":"current"}` show:

```JSON
{
  "jsonrpc": "2.0",
  "result": {
    "current_media": {
      "category": "",
      "duration": 154.2,
      "out": 154.2,
      "seek": 0.0,
      "source": "/opt/tv-media/clip.mp4"
    },
    "index": 39,
    "play_mode": "playlist",
    "played_sec": 67.80771999300123,
    "remaining_sec": 86.39228000699876,
    "start_sec": 24713.631999999998,
    "start_time": "06:51:53.631"
  },
  "id": 1
}
```

When you are in playlist mode and jumping forward or backwards in time, the time shift will be saved so the playlist is still in sync. But have in mind, that then maybe your playlist gets to short. When you are not resetting the state, it will reset on the next day automatically.
