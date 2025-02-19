**ffplayout**
================

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

![player](/docs/images/player.png)

ffplayout is a 24/7 broadcasting solution. It can playout a folder containing audio or video clips, or play a *JSON* playlist for each day, keeping the current playlist editable.

The application is mostly designed to run as system service on Linux. But in general it should run on any platform supported by Rust.

Check the [releases](https://github.com/ffplayout/ffplayout/releases/latest) for pre compiled version.

### Features

- start program with [web based frontend](/frontend/), or run playout in foreground mode without frontend
- dynamic playlist
- replace missing playlist or clip with single filler or multiple fillers from folder, if no filler exists, create dummy clip
- playing clips in [watched](/docs/folder_mode.md) folder mode
- send emails with error message
- overlay a logo
- overlay text, controllable through [web frontend](/frontend/) (needs ffmpeg with libzmq and enabled JSON RPC server)
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
- [live ingest](/docs/live_ingest.md)
- image source (will loop until out duration is reached)
- extra audio source, has priority over audio from video (experimental *)
- [multiple audio tracks](/docs/multi_audio.md) (experimental *)
- [Stream Copy](/docs/stream_copy.md) mode (experimental *)
- [custom filters](/docs/custom_filters.md) globally in config, or in playlist for specific clips
- import playlist from text or m3u file, with CLI or frontend
- audio only, for radio mode (experimental *)
- generate playlist based on [template](/docs/playlist_gen.md) (experimental *)
- During playlist import, all video clips are validated and, if desired, checked to ensure that the audio track is not completely muted.
- run multiple channels (experimental *)
- vtt [subtitle](/docs/closed_captions.md) in HLS mode (experimental *)

For preview stream, read: [/docs/preview_stream.md](/docs/preview_stream.md)

**\* Experimental features do not guarantee the same stability and may fail under unusual circumstances. Code and configuration options may change in the future.**

### Requirements

- RAM and CPU depends on video resolution, minimum 4 _dedicated_ threads and 3GB RAM for 720p are recommend
- **ffmpeg** v5.0+ and **ffprobe** (**ffplay** if you want to play on desktop), version 7.2+ for vtt
- if you want to overlay dynamic text, ffmpeg needs to have **libzmq**

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
If you are in playlist mode and move backwards or forwards in time, the time shift is saved so the playlist is still in sync. Bear in mind, however, that this may make your playlist too short. If you do not reset it, it will automatically reset the next day.

## **Warning**

(Endless) streaming over multiple days will only work if config has a **day_start** value and the **length** value is **24 hours**. If you only need a few hours for each day, use a *cron* job or something similar.

## Note
This project includes the DejaVu font, which are licensed under the [Bitstream Vera Fonts License](/assets/FONT_LICENSE.txt).
ve.com/ffplayout/backers.svg?width=800&button=true)](https://opencollective.com/ffplayout)
