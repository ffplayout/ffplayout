**ffplayout**
================

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

![player](/docs/images/player.png)

ffplayout is a 24/7 broadcasting solution. It can playout a folder containing audio or video clips, or play a *JSON* playlist for each day, keeping the current playlist editable.

The application is mostly designed to run as a system service on Linux. In general it should run on any platform supported by Rust and FFmpeg.

Check the [releases](https://github.com/ffplayout/ffplayout/releases/latest) for prebuilt packages.

### Features

- start program with [web based frontend](/frontend/), or run playout in foreground mode without frontend
- dynamic playlist
- replace missing playlist or clip with single filler or multiple fillers from folder, if no filler exists, create dummy clip
- playing clips in [watched](/docs/folder_mode.md) folder mode
- send emails with error message
- overlay a logo
- overlay text, controllable through [web frontend](/frontend/)
- loop playlist infinitely
- [remote source](/docs/remote_source.md)
- trim last clip, to get full 24 hours
- when playlist is not 24 hours long, loop fillers until time is full
- set a custom day start, for example from 06:00 to 06:00 instead of midnight to midnight
- normal system requirements and no special tools beyond FFmpeg libraries
- no GPU power is needed
- stream to server or play on desktop
- log to channel log files, mail queues, or color output to console
- conform audio and video, if is necessary to match output stream:
  - letterbox or pillarbox to fit aspect
  - change fps
  - fit target resolution
  - add silence if audio duration is too short
  - hold the last frame if video duration is too short
- [output](/docs/output.md):
  - **stream**
  - **desktop**
  - **HLS**
- RTMP [live ingest](/docs/live_ingest.md)
- image source (will loop until out duration is reached)
- import playlist from text or m3u file, with CLI or frontend
- generate playlist based on [template](/docs/playlist_gen.md)
- During playlist import, all video clips are validated and, if desired, checked to ensure that the audio track is not completely muted.
- run multiple channels (experimental *)
- WebVTT [subtitles](/docs/closed_captions.md) in HLS mode (experimental *)

**\* Experimental features do not guarantee the same stability and may fail under unusual circumstances. Code and configuration options may change in the future.**

### Requirements

- RAM and CPU usage depends on video resolution; at least 4 dedicated threads and 3GB RAM for 720p are recommended
- **FFmpeg** v7.0+ development libraries are required; FFmpeg builds before 7.2 can use WebVTT subtitles, but may not support custom HLS subtitle display names

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
            "source": "/Media/clip2.mp4"
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
            "source": "/Media/image1.jpg"
        }, {
            "in": 0,
            "out": 230.30,
            "duration": 230.30,
            "source": "/Media/image2.jpg"
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
If you are in playlist mode and move backwards or forwards in time, the time shift is saved so the playlist is still in sync. Bear in mind, however, that this may make your playlist too short. If you do not reset it, it will automatically reset the next day.

## **Warning**

(Endless) streaming over multiple days will only work if config has a **day_start** value and the **length** value is **24 hours**. If you only need a few hours for each day, use a *cron* job or something similar.
