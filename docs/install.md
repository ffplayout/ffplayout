### Install ffplayout

ffplayout provides ***.deb** amd ***.rpm** packages, which makes it more easy to install and use, but there is still some steps to do.

1. download the latest ffplayout from [release](https://github.com/ffplayout/ffplayout/releases/latest) page.
2. install it with `apt install /tmp/ffplayout_<VERSION>_amd64.deb`
3. install ffmpeg/ffprobe, or compile and copy it to **/usr/local/bin/**
4. activate systemd services:
    - `systemctl enable ffplayout`
    - `systemctl enable --now ffpapi`
5. add admin user to ffpapi:
    - `ffpapi -a`
6. use a revers proxy for SSL, Port is **8787**.
7. login with your browser, address without proxy would be: **http://[IP ADDRESS]:8787**

Default location for playlists and media files are: **/var/lib/ffplayout/**. If you need to change them, the media storage folder needs a symlink to **/usr/share/ffplayout/public/**.

When you don't need the frontend and API, skip enable the systemd service **ffpapi**.

When playlists are created and the ffplayout output is configured, you can start the process: `systemctl start ffplayout`, or click start in frontend.

If you want to configure ffplayout over terminal, you can edit **/etc/ffplayout/ffplayout.yml**.

### Manual Install
-----

- copy the binary to `/usr/bin/`
- copy **assets/ffplayout.yml** to `/etc/ffplayout`
- create folder `/var/log/ffplayout`
- create system user **ffpu**
- give ownership from `/etc/ffplayout` and `/var/log/ffplayout` to **ffpu**
- copy **assets/ffplayout.service** to `/etc/systemd/system`
- activate service and run it: `systemctl enable --now ffplayout`
