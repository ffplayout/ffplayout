### Install ffplayout

ffplayout provides ***.deb** and ***.rpm** packages, which makes it more easy to install and use, but there is still some steps to do.

1. download the latest ffplayout from [release](https://github.com/ffplayout/ffplayout/releases/latest) page and place the package in the **/tmp** folder.
2. install it with `apt install /tmp/ffplayout_<VERSION>_amd64.deb`
3. install ffmpeg/ffprobe, or compile and copy it to **/usr/local/bin/**
4. activate systemd services:
    - `systemctl enable ffplayout`
5. initial defaults and add global admin user:
    - `sudo -u ffpu ffplayout -i`
6. start ffplayout:
    - `systemctl start ffplayout`
7. use a revers proxy for SSL, Port is **8787**.
8. login with your browser, address without proxy would be: **http://[IP ADDRESS]:8787**

Default location for playlists and media files are: **/var/lib/ffplayout/**.

### Manual Install
-----

- install ffmpeg/ffprobe, or compile and copy it to **/usr/local/bin/**
- download the latest archive from [release](https://github.com/ffplayout/ffplayout/releases/latest) page
- copy the ffplayout binary to `/usr/bin/`
- copy **assets/ffplayout.yml** to `/etc/ffplayout`
- create folder `/var/log/ffplayout`
- create system user **ffpu**
- give ownership from `/etc/ffplayout` and `/var/log/ffplayout` to **ffpu**
- copy **assets/ffplayout.service** to `/etc/systemd/system`
- copy **assets/ffplayout.1.gz** to `/usr/share/man/man1/`
- copy **public** folder to `/usr/share/ffplayout/`
- activate service and run it: `systemctl enable --now ffplayout`
