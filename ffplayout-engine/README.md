**ffplayout-engine**
================

Start with Arguments
-----

ffplayout also allows the passing of parameters:

```
OPTIONS:
    -c, --config <CONFIG>             File path to ffplayout.conf
    -f, --folder <FOLDER>             Play folder content
    -g, --generate <YYYY-MM-DD>...    Generate playlist for date or date-range, like: 2022-01-01 - 2022-01-10:
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
