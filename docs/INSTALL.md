**ffplayout-engine Installation**
================

Here are a description on how to install *ffplayout engine* on a standard Linux server.

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
- run `make` (virtualenv is required)
- run `sudo make install USER=www-data`, use any other user which need write access
- create playlists folder, in that format: **/playlists/year/month**
- set variables in config file to your needs
- use `docs/gen_playlist_from_subfolders.sh /path/to/mp4s/` as a starting point for your playlists (path in script needs to change)
- activate service and start it: `sudo systemctl enable ffplayout-engine && sudo systemctl start ffplayout-engine`

Cleanup
-----
- run `make clean` to remove the virtual environment

Deinstallation
-----
- run `sudo make uninstall` it will remove all created folders (also the **ffplayout.yml** configuration file!)

Manual Installation
-----
The routine with `make` build a virtual environment with all dependencies, and install ffplayout to **/opt/ffplayout-engine**. If you do not want to install to this path, or you want to install the dependencies globally, you can do everything by hand.

Just copy the project where you want to have it, run inside `pip3 install -r requirements.txt`. For logging you have to create the folder **ffplayout** under **/var/log/**, or adjust the settings in config. **ffplayout.yml** have to go to **/etc/ffplayout/**, or should stay in same folder.

If you want to use the systemd service, edit the service file in **docs/ffplayout-engine.service**, copy it to **/etc/systemd/system/** and activate it with: `sudo systemctl enable ffplayout-engine`.

Using it Without Installation
-----
Of course you can just run it too. Install only the dependencies from **requirements.txt** and run it with **python ffplayout.py [parameters]**.
