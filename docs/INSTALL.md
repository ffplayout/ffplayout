**ffplayout-engine installation**
================

Here are a description on how to install *ffplayout engine* on a standard linux server.

Requirements
-----
- python version 3.6+
- **ffmpeg v4.2+** and **ffprobe**
- systemd (if ffplayout should run as a daemon)

Installation
-----
- install ffmpeg, ffprobe (and ffplay if you need the preview mode)
- clone repo: `git clone https://github.com/ffplayout/ffplayout-engine.git`
- `cd ffplayout-engine`
- run `make`
- run `sudo make install USER=www-data`, use any other user which need write access
- create playlists folder, in that format: **/playlists/year/month**
- set variables in config file to your needs
- use `docs/gen_playlist_from_subfolders.sh /path/to/mp4s/` as a starting point for your playlists (path in script needs to change)
- activate service and start it: `sudo systemctl enable ffplayout && sudo systemctl start ffplayout`

Cleanup
-----
- run `make clean` to remove virtual environment

Deinstallation
-----
- run `sudo make uninstall` it will remove all created folders (also the **ffplayout.yml** configuration file!)
