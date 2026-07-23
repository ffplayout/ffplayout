### Install ffplayout

**Note:** This is the official and supported way.

ffplayout provides ***.deb** and ***.rpm** packages, which makes it easier to install and use, but there are still some steps to follow.

1. Download the latest ffplayout from the [release](https://github.com/ffplayout/ffplayout/releases/latest) page and place the package in the **/tmp** folder
2. Install it with `apt install /tmp/ffplayout_<VERSION>_amd64.deb`
3. Install FFmpeg 7.1+ runtime libraries and tools (`ffmpeg`, `ffprobe`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, and `libswresample`), or install FFmpeg to **/usr/local/**. For Windows you can use this shared [FFmpeg libraries](https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip).
4. Start the service, open **http://[IP ADDRESS]:8787**, and complete the first-time setup in the browser. It creates the global settings and first global admin.
5. Use a reverse proxy for SSL before exposing ffplayout to the Internet.

The package installation creates a service user named **ffpu**. The configured
log, playlist, public, and storage directories must be writable by that user.

For headless installations, the interactive CLI setup remains available as an
alternative to the web setup:

```bash
sudo -u ffpu ffplayout -i
```

The web setup accepts directory paths only during the first initialization.
To change log, playlist, public, or storage paths later, run `ffplayout -i`
locally as the service user.

### Manual Install

**Note:** This is for advanced users only.

- Install FFmpeg 7.1+ runtime libraries and tools, or compile and install them to **/usr/local/**
- Download the latest archive from the [release](https://github.com/ffplayout/ffplayout/releases/latest) page
- Copy the ffplayout binary to `/usr/bin/`
- Copy **assets/ffplayout.conf** to `/etc/ffplayout`
- Create the folder `/var/log/ffplayout`
- Create the system user **ffpu**
- Give ownership of `/etc/ffplayout` and `/var/log/ffplayout` to **ffpu**
- Create the storage, playlist, and public directories selected during setup and make them writable by **ffpu**
- Copy **assets/ffplayout.service** to `/etc/systemd/system`
- Copy **assets/ffplayout.1.gz** to `/usr/share/man/man1/`
- Copy **assets/dummy.vtt**, **assets/logo.png** to `/usr/share/ffplayout/`
- Activate the service and run it: `systemctl enable --now ffplayout`
- Open `http://[IP ADDRESS]:8787` and complete first-time setup. Alternatively, run `sudo -u ffpu ffplayout -i` before starting the service.
