**ffplayout-engine**
================

Installation under Linux
-----

- copy the binary to `/usr/bin/`
- copy **assets/ffplayout.yml** to `/etc/ffplayout`
- create folder `/var/log/ffplayout`
- create system user **ffpu**
- give ownership from `/etc/ffplayout` and `/var/log/ffplayout` to **ffpu**
- copy **assets/ffplayout.service** to `/etc/systemd/system`
- activate service and run it: `systemctl enable --now ffplayout`

You can also install the [released](https://github.com/ffplayout/ffplayout/releases/latest) ***.deb** or ***.rpm** package.

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
