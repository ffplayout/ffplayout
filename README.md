**ffplayout-engine**
================

[![made-with-python](https://img.shields.io/badge/Made%20with-Python-1f425f.svg)](https://www.python.org/)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

The purpose with ffplayout is to provide a 24/7 broadcasting solution that plays a *json* playlist for every day, while keeping the current playlist editable.

**Check [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend): web-based GUI for ffplayout**

**Features**
-----

- have all values in a separate config file
- dynamic playlist
- replace missing playlist or clip with a dummy clip
- playing clips from [watched folder](https://github.com/ffplayout/ffplayout-engine/wiki/Watch-Folder)
- send emails with error message
- overlay a logo
- overlay text, controllable through [messenger](https://github.com/ffplayout/messenger) over libzmq
- **EBU R128 loudness** normalization (single pass) (experimental)
- loop clip in playlist which `out` value is higher then its `duration`, see also [Loop Clip](https://github.com/ffplayout/ffplayout-engine/wiki/Loop-Clip)
- loop playlist infinitely
- trim and fade the last clip, to get full 24 hours
- when playlist is not 24 hours long, loop filler clip until time is full
- set custom day start, so you can have playlist for example: from 6am to 6am, instate of 0am to 12pm
- normal system requirements and no special tools
- no GPU power is needed
- stream to server or play on desktop
- on posix systems ffplayout can reload config with *SIGHUP*
- logging to files, or colored output to console
- add filters to input, if is necessary to match output stream:
  - **yadif** (deinterlacing)
  - **pad** (letterbox or pillarbox to fit aspect)
  - **fps** (change fps)
  - **scale** (fit target resolution)
  - **aevalsrc** (if video have no audio)
  - **apad** (add silence if audio duration is to short)
  - **tpad** (add black frames if video duration is to short)
- add custom [filters](https://github.com/ffplayout/ffplayout-engine/tree/master/ffplayout/filters)
- add custom [arguments](https://github.com/ffplayout/ffplayout-engine/tree/master/ffplayout/config)
- different types of [output](https://github.com/ffplayout/ffplayout-engine/wiki/Outputs):
  - **stream**
  - **desktop**
  - **HLS**
  - **custom**
- Multi Channel

Requirements
-----

- python version 3.6+
- python module **watchdog** (only when `playlist_mode: False`)
- python module **colorama** if you are on windows
- python modules **PyYAML**, **requests**, **supervisor**
- **ffmpeg v4.2+** and **ffprobe** (**ffplay** if you want to play on desktop)
- if you want to overlay text, ffmpeg needs to have **libzmq**
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

The playlist can be extend, to use custom attributes in your [filters](/ffplayout/filters/).

**Warning**
-----

(Endless) streaming over multiple days will only work when config have **day_start** value and the **length** value is **24 hours**. If you need only some hours for every day, use a *cron* job, or something similar.

Remote source from URL
-----

You can use sources from remote URL in that way:

```json
...
        {
            "in": 0,
            "out": 149,
            "duration": 149,
            "source": "https://example.org/big_buck_bunny.webm"
        }
```

But be careful with it, better test it multiple times!

More informations in [Wiki](https://github.com/ffplayout/ffplayout-engine/wiki/Remote-URL-Source)

Installation
-----

Check [INSTALL.md](docs/INSTALL.md)

Start with Arguments
-----

ffplayout also allows the passing of parameters:

- `-c, --config` use given config file
- `-f, --folder` use folder for playing
- `-l, --log` for user-defined log path, *none* for console output
- `-i, --loop` loop playlist infinitely
- `-m, --mode` set output mode: **desktop**, **hls**, **stream**, ...
- `-p, --playlist` for playlist file
- `-s, --start` set start time in *hh:mm:ss*, *now* for start with first
- `-t, --length` set length in *hh:mm:ss*, *none* for no length check

You can run the command like:

```SHELL
./ffplayout.py -l none -p ~/playlist.json -d -s now -t none -m desktop
```

Multi Channel
-----

For streaming multiple channels you need to use python supervisorD. A systemd [service](/docs/ffplayout-engine-multichannel.service) for it you found in docs folder.

To configure every channel create your ffplayout yaml configs under **/etc/ffplayout** with naming them **ffplayout-001.yml**, **ffplayout-002.yml** and so on. Then copy and edit the supervisor config [/supervisor/conf.d/engine-001.conf](/supervisor/conf.d/engine-001.conf) with the same numbering.
