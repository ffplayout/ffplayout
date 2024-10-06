### Install ffplayout

**Note:** This is the official and supported way.

ffplayout provides ***.deb** and ***.rpm** packages, which makes it easier to install and use, but there are still some steps to follow.

1. Download the latest ffplayout from the [release](https://github.com/ffplayout/ffplayout/releases/latest) page and place the package in the **/tmp** folder
2. Install it with `apt install /tmp/ffplayout_<VERSION>_amd64.deb`
3. Install ffmpeg/ffprobe, or compile and copy them to **/usr/local/bin/**
4. Initialize the defaults and add a global admin user: `sudo -u ffpu ffplayout -i`
5. Use a reverse proxy for SSL; the port is **8787**
6. Log in with your browser. The address without a proxy would be: **http://[IP ADDRESS]:8787**

### Manual Install

**Note:** This is for advanced users only.

- Install ffmpeg/ffprobe, or compile and copy them to **/usr/local/bin/**
- Download the latest archive from the [release](https://github.com/ffplayout/ffplayout/releases/latest) page
- Copy the ffplayout binary to `/usr/bin/`
- Copy **assets/ffplayout.yml** to `/etc/ffplayout`
- Create the folder `/var/log/ffplayout`
- Create the system user **ffpu**
- Give ownership of `/etc/ffplayout` and `/var/log/ffplayout` to **ffpu**
- Copy **assets/ffplayout.service** to `/etc/systemd/system`
- Copy **assets/ffplayout.1.gz** to `/usr/share/man/man1/`
- Copy the **public** folder to `/usr/share/ffplayout/`
- Activate the service and run it: `systemctl enable --now ffplayout`
- Initialize the defaults and add a global admin user: `sudo -u ffpu ffplayout -i`
