**ffplayout_engine Installation**
================

Here are a description on how to install *ffplayout engine* on a standard Linux server.

Requirements
-----

- python version 3.6+
- **ffmpeg v4.2+** and **ffprobe**

Installation
-----

- install **ffmpeg**, **ffprobe** (and **ffplay** if you need the preview mode)
- clone repo to **/opt/**: `git clone https://github.com/ffplayout/ffplayout_engine.git`
- `cd /opt/ffplayout_engine`
- create virtual environment: `virtualenv -p python3 venv`
- run `source ./venv/bin/activate`
- install dependencies: `pip3 install -r requirements.txt`
- create logging folder: **/var/log/ffplayout**
- create playlists folder, in that format: **/playlists/year/month**
- create folder for media storage: **/tv-media**
- set variables in config file to your needs

Single Channel Setup
-----

**systemd** is required

- copy **docs/ffplayout_engine.service** to **/etc/systemd/system/**
- copy **ffplayout.yml** to **/etc/ffplayout/**
- change user and group in service file (for example to **www-data**)
- activate service: `sudo systemctl enable ffplayout_engine`
- edit **/etc/ffplayout/ffplayout.yml**
- when playlists are exists, run service: `sudo systemctl start ffplayout_engine`

Multi Channel Setup
-----

- copy **docs/ffplayout_engine-multichannel.service** to **/etc/systemd/system/**
- change user and group in service file (for example to **www-data**)
- copy **ffplayout.yml** to **/etc/ffplayout/ffplayout-001.yml**
- copy **docs/supervisor** folder to **/etc/ffplayout/**
- every channel needs its own engine config **ffplayout-002.yml**, **ffplayout-003.yml**, etc.
- every channel needs also its own service file under **/etc/ffplayout/supervisor/config.d**
- create for every channel a subfolder for logging: **/var/log/ffplayout/channel-001**,  **/var/log/ffplayout/channel-002**, etc.
- edit **/etc/ffplayout/ffplayout-00*.yml**
- when you want to use the web frontend, create only the first channel and the other ones in the frontend
- activate service: `sudo systemctl enable ffplayout_engine-multichannel`
- when playlists are exists, run service: `sudo systemctl start ffplayout_engine-multichannel`

Using it Without Installation
-----
Of course you can just run it too. Install only the dependencies from **requirements.txt** and run it with **python ffplayout.py [parameters]**.
