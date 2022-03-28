**ffplayout-rs**
================

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

## Attention:
Soon this code willbe merged in [ffplayout_engine](https://github.com/ffplayout/ffplayout_engine)

The main purpose of ffplayout is to provide a 24/7 broadcasting solution that plays a *json* playlist for every day, while keeping the current playlist editable.

**Check [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend): web-based GUI for ffplayout**

**Features**
-----

- have all values in a separate config file
- dynamic playlist
- replace missing playlist or clip with a dummy clip
- playing clips from [watched folder](https://github.com/ffplayout/ffplayout_engine/wiki/Watch-Folder)
- send emails with error message
- overlay a logo
- overlay text, controllable through [messenger](https://github.com/ffplayout/messenger) or [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend) (needs ffmpeg with libzmq)
- EBU R128 loudness normalization (single pass)
- loop playlist infinitely
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
- output:
  - **stream**
  - **desktop**

Requirements
-----

- RAM and CPU depends on video resolution, minimum 4 threads and 3GB RAM for 720p are recommend

JSON Playlist Example
-----

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
            "source": "/Media/clip2.mp4"
        }, {
            "in": 0,
            "out": 114.72,
            "duration": 114.72,
            "source": "/Media/clip3.mp4",
            "category": "advertisement"
        }, {
            "in": 0,
            "out": 2531.36,
            "duration": 2531.36,
            "source": "/Media/clip4.mp4",
            "category": ""
        }
    ]
}
```

**If you need a simple playlist generator check:** [playlist-generator](https://github.com/ffplayout/playlist-generator)

**Warning**
-----

(Endless) streaming over multiple days will only work when config have **day_start** value and the **length** value is **24 hours**. If you need only some hours for every day, use a *cron* job, or something similar.

Remote source from URL
-----

You can use sources from remote URL in that way:

```json
        {
            "in": 0,
            "out": 149,
            "duration": 149,
            "source": "https://example.org/big_buck_bunny.webm"
        }
```

But be careful with it, better test it multiple times!

More informations in [Wiki](https://github.com/ffplayout/ffplayout_engine/wiki/Remote-URL-Source)

Installation
-----

Copy the binary to `/usr/local/bin/`

Start with Arguments
-----

ffplayout also allows the passing of parameters:

- `-c, --config <CONFIG>`          file path to ffplayout.conf
- `-f, --folder <FOLDER>`          play folder content
- `-h, --help`                     Print help information
- `-i, --infinit`                  loop playlist infinitely
- `-l, --log <LOG>`                file path for logging
- `-m, --play-mode <PLAY_MODE>`    playing mode: folder, playlist
- `-o, --output <OUTPUT>`          set output mode: desktop, hls, stream
- `-p, --playlist <PLAYLIST>`      path from playlist
- `-s, --start <START>`            start time in 'hh:mm:ss', 'now' for start with first
- `-t, --length <LENGTH>`          set length in 'hh:mm:ss', 'none' for no length check
- `-v, --volume <VOLUME>`          set audio volume
- `-V, --version`                  Print version information


You can run the command like:

```Bash
./ffplayout.py -l none -p ~/playlist.json -o desktop
```
