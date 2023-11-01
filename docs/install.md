### Install ffplayout

ffplayout provides ***.deb** and ***.rpm** packages, which makes it more easy to install and use, but there is still some steps to do.

1. download the latest ffplayout from [release](https://github.com/ffplayout/ffplayout/releases/latest) page and place the package in the **/tmp** folder.
2. install it with `apt install /tmp/ffplayout_<VERSION>_amd64.deb`
3. install ffmpeg/ffprobe, or compile and copy it to **/usr/local/bin/**
4. activate systemd services:
    - `systemctl enable ffplayout`
    - `systemctl enable --now ffpapi`
5. add admin user to ffpapi:
    - `ffpapi -a`
6. use a revers proxy for SSL, Port is **8787**.
7. login with your browser, address without proxy would be: **http://[IP ADDRESS]:8787**

Default location for playlists and media files are: **/var/lib/ffplayout/**.

When you don't need the frontend and API, skip enable the systemd service **ffpapi**.

When playlists are created and the ffplayout output is configured, you can start the process: `systemctl start ffplayout`, or click start in frontend.

If you want to configure ffplayout over terminal, you can edit **/etc/ffplayout/ffplayout.yml**.

### Manual Install
-----

- install ffmpeg/ffprobe, or compile and copy it to **/usr/local/bin/**
- download the latest archive from [release](https://github.com/ffplayout/ffplayout/releases/latest) page
- copy the ffplayout and ffpapi binary to `/usr/bin/`
- copy **assets/ffplayout.yml** to `/etc/ffplayout`
- create folder `/var/log/ffplayout`
- create system user **ffpu**
- give ownership from `/etc/ffplayout` and `/var/log/ffplayout` to **ffpu**
- copy **assets/ffpapi.service**, **assets/ffplayout.service** and **assets/ffplayout@.service** to `/etc/systemd/system`
- copy **assets/11-ffplayout** to `/etc/sudoers.d/`
- copy **assets/ffpapi.1.gz** and **assets/ffplayout.1.gz** to `/usr/share/man/man1/`
- copy **public** folder to `/usr/share/ffplayout/`
- activate service and run it: `systemctl enable --now ffpapi ffplayout`
