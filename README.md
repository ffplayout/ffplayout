**ffplayout-engine**
================
[![made-with-python](https://img.shields.io/badge/Made%20with-Python-1f425f.svg)](https://www.python.org/)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

The purpose with ffplayout is to provide a 24/7 broadcasting solution that plays a *json* playlist for every day, while keeping the current playlist editable.

#### Check [ffplayout-gui](https://github.com/ffplayout/ffplayout-gui): web-based GUI for ffplayout.

Features
-----

- have all values in a separate config file
- dynamic playlist
- replace missing playlist or clip with a dummy clip
- playing clips from [watched folder](https://github.com/ffplayout/ffplayout-engine/wiki/Watch-Folder)
- send emails with error message
- overlay a logo
- overlay scrolling text
- trim and fade the last clip, to get full 24 hours
- set custom day start, so you can have playlist for example: from 6am to 6am, instate of 0am to 12pm
- copy mode, for more info's take a look in the [Wiki](https://github.com/ffplayout/ffplayout-engine/wiki/Copy-Mode)
- normal system requirements and no special tools
- no GPU power is needed
- stream to server or play on desktop

Requirements
-----
- python version 3.5+
- python module **watchdog** (onyl when `playlist_mode = False`)
- **ffmpeg** and **ffprobe** (**ffplay** if you want to play on desktop)
- RAM and CPU depends on video resolution, minimum 4 threads and 3GB RAM for 720p are recommend

JSON Playlist Example
-----

```json
{
    "channel": "Test 1",
    "date": "2019-03-05",
    "length": "24:00:00.000",
    "program": [{
            "in": 0,
            "out": 647.68,
            "duration": 647.68,
            "source": "/Media/clip1.mp4",
        }, {
            "in": 0,
            "out": 149,
            "duration": 149,
            "source": "/Media/clip2.mp4",
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

`"length"` are optional, when you leave **day_start** in config blank, length check will be ignored and the playlist starts from the begin, without time awareness. If you leave **length** blank, the validation will not check if the real length of the playlist will match the length value.

#### Warning:
(Endless) streaming over multiple days will only work when config have **day_start** value and the **length** value of the playlist is **24 hours**. If you need only some hours for every day, use a *cron* job, or something similar.

Source from URL / Live Stream
-----
You can use sources from url or live stream in that way:

```json
...
        {
            "in": 0,
            "out": 149,
            "duration": 149,
            "source": "https://example.org/big_buck_bunny.webm"
        },
...
        {
            "in": 0,
            "out": 2531.36,
            "duration": 0,
            "source": "rtmp://example.org/live/stream"
        }
...
```
But be careful with it, better test it multiple times!

More informations in [Wiki](https://github.com/ffplayout/ffplayout-engine/wiki/URL---Live-Source)

Installation
-----
- install ffmpeg, ffprobe (and ffplay if you need the preview mode)
- copy ffplayout.py to **/usr/local/bin/**
- copy ffplayout.conf to **/etc/ffplayout/**
- create folder with correct permissions for logging (check config)
- copy ffplayout.service to **/etc/systemd/system/**
- change user in **/etc/systemd/system/ffplayout.service**
- create playlists folder, in that format: **/playlists/year/month**
- set variables in config file to your needs
- use **get_playlist_from_subfolders.sh /path/to/mp4s/** as a starting point for your playlists (path in script needs to change)
- activate service and start it: **sudo systemctl enable ffplayout && sudo systemctl start ffplayout**

Start with Arguments
-----
ffplayout also allows the passing of parameters:
- `-l, --log` for user-defined log file
- `-f, --file` for playlist file

The entire command could look like this:

```
python3 ffplayout.py -l ~/ffplayout.log -f ~/playlist.json
```

Preview Mode on Desktop
-----
For playing on desktop set `preview = True` in config under `[OUT]`.
