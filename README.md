**ffplayout-engine**
================

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

The main purpose of ffplayout is to provide a 24/7 broadcasting solution that plays a *json* playlist for every day, while keeping the current playlist editable.

**Check [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend): web-based GUI for ffplayout**

**Features**
-----

- have all values in a separate config file
- dynamic playlist
- replace missing playlist or clip with a dummy clip
- playing clips in [watched](/docs/folder_mode.md) folder mode
- send emails with error message
- overlay a logo
- overlay text, controllable through [messenger](https://github.com/ffplayout/messenger) or [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend) (needs ffmpeg with libzmq)
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
- JSON RPC server, for getting infos about current playing and controlling
- [live ingest](/docs/live_ingest.md)

Requirements
-----

- RAM and CPU depends on video resolution, minimum 4 threads and 3GB RAM for 720p are recommend
- **ffmpeg** v4.2+ and **ffprobe** (**ffplay** if you want to play on desktop)
- if you want to overlay text, ffmpeg needs to have **libzmq**

-----

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
            "source": "https://example.org/big_buck_bunny.webm",
            "category": ""
        }
    ]
}
```

**Warning**
-----

(Endless) streaming over multiple days will only work when config have **day_start** value and the **length** value is **24 hours**. If you need only some hours for every day, use a *cron* job, or something similar.

-----

HLS output
-----

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

JSON RPC
-----

The ffplayout engine can run a JSON RPC server. A request show look like:

```Bash
curl -X POST -H "Content-Type: application/json" -H "Authorization: ---auth-key---" \
    -d '{"jsonrpc": "2.0", "method": "player", "params":{"control":"next"}, "id":1 }' \
    127.0.0.1:7070
```

At the moment this comments are possible:

```Bash
'{"jsonrpc": "2.0", "method": "player", "params":{"media":"current"}, "id":1 }'  # get infos about current clip
'{"jsonrpc": "2.0", "method": "player", "params":{"media":"next"}, "id":2 }'  # get infos about next clip
'{"jsonrpc": "2.0", "method": "player", "params":{"media":"last"}, "id":3 }'  # get infos about last clip
'{"jsonrpc": "2.0", "method": "player", "params":{"control":"next"}, "id":4 }'   # jump to next clip
'{"jsonrpc": "2.0", "method": "player", "params":{"control":"back"}, "id":5 }'   # jump to last clip
'{"jsonrpc": "2.0", "method": "player", "params":{"control":"reset"}, "id":6 }'  # reset playlist to old state

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

-----

Installation
-----

Copy the binary to `/usr/local/bin/`

Start with Arguments
-----

ffplayout also allows the passing of parameters:

```
OPTIONS:
    -c, --config <CONFIG>             File path to ffplayout.conf
    -f, --folder <FOLDER>             Play folder content
    -g, --generate <YYYY-MM-DD>...    Generate playlist for date. Date-range is possible, like:
                                      2022-01-01 - 2022-01-10.
    -h, --help                        Print help information
    -i, --infinit                     Loop playlist infinitely
    -l, --log <LOG>                   File path for logging
    -m, --play-mode <PLAY_MODE>       Playing mode: folder, playlist
    -o, --output <OUTPUT>             Set output mode: desktop, hls, stream
    -p, --playlist <PLAYLIST>         Path from playlist
    -s, --start <START>               Start time in 'hh:mm:ss', 'now' for start with first
    -t, --length <LENGTH>             Set length in 'hh:mm:ss', 'none' for no length check
    -v, --volume <VOLUME>             Set audio volume
    -V, --version                     Print version information

```


You can run the command like:

```Bash
./ffplayout -l none -p ~/playlist.json -o desktop
```
