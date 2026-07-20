**ffplayout**
================

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

![player](/docs/images/player.png)

ffplayout is a 24/7 broadcasting solution. It can playout a folder containing video clips, or play a *JSON* playlist for each day, keeping the current playlist editable.

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
- CPU-based processing; a GPU is not required
- log to channel log files, mail queues, or color output to console
- conform audio and video, if is necessary to match output stream:
  - letterbox or pillarbox to fit aspect
  - change fps
  - fit target resolution
  - add silence if audio duration is too short
  - hold the last frame if video duration is too short
- [output](/docs/output.md): **stream**, **desktop**, and **HLS**
- RTMP [live ingest](/docs/live_ingest.md)
- image source (will loop until out duration is reached)
- import playlist from text or m3u file, with CLI or frontend
- generate playlist based on [template](/docs/playlist_gen.md)
- run an [external task](/docs/external_tasks.md) when a clip starts
- During playlist import, all video clips are validated and, if desired, checked to ensure that the audio track is not completely muted.
- run multiple channels (experimental *)
- WebVTT [subtitles](/docs/closed_captions.md) in HLS mode (experimental *)

**\* Experimental features do not guarantee the same stability and may fail under unusual circumstances. Code and configuration options may change in the future.**

### Requirements

- RAM and CPU usage depends on video resolution; at least 4 dedicated threads and 3GB RAM for 720p are recommended
- **FFmpeg** v7.0+ development libraries are required; the maximum supported version follows `ffmpeg-next` and is currently FFmpeg 8.1. Builds with `libavformat` before v61.9.100 can use WebVTT subtitles, but may not support custom HLS subtitle display names

### Install

Check [install](docs/install.md) for details about how to install ffplayout.

### Quick Start

1. Install a package from the [latest release](https://github.com/ffplayout/ffplayout/releases/latest).
2. Start the ffplayout service as described in the [installation guide](docs/install.md).
3. Open `http://<server-address>:8787` and complete the first-time setup. It creates the global settings and the first global admin.

HLS is the default output mode. For production delivery, serve the generated HLS files through nginx, another web server, or a CDN; ffplayout's built-in HTTP route is intended for previewing.

---

### JSON Playlist Example

```json
{
    "channel": "My Channel",
    "date": "2026-07-15",
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
            "source": "/Media/image.jpg"
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

## Day-Long Playlists

For continuous, day-based playout across multiple days, configure a **day_start** value and set **length** to **24 hours**. For shorter scheduled runs, start ffplayout with a scheduler such as `cron` or systemd timers.
