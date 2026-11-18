### Install ffplayout

**Note:** This is the official and supported way.

ffplayout provides ***.deb** and ***.rpm** packages, which makes it easier to install and use, but there are still some steps to follow.

1. Download the latest ffplayout from the [release](https://github.com/ffplayout/ffplayout/releases/latest) page and place the package in the **/tmp** folder
2. Install it with `apt install /tmp/ffplayout_<VERSION>_amd64.deb`
3. Install FFmpeg 7.1+ runtime libraries and tools (`ffmpeg`, `ffprobe`, `libavcodec`, `libavformat`, `libavutil`, `libswscale`, and `libswresample`), or install FFmpeg to **/usr/local/**. For Windows you can use this shared [FFmpeg libraries](https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip).
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

### Export and restore settings

`--dump-config` exports all persisted settings to `ffplayout.json` in the current
directory. It includes global settings, channels, outputs and encoding links,
recording settings, live inputs, source options, text presets, saved graphics
layers, users with password hashes, and channel permissions.

```bash
ffplayout --db /path/to/ffplayout.db --dump-config
```

**Keep exports private:** they contain credentials, including SMTP passwords,
notification tokens, stream URLs where configured, and user password hashes.
On Unix, export files are written atomically with permissions `0600`; an existing export at the same filename is replaced.

Restore into a **new database file**:

```bash
ffplayout --db ./restored.db --import-config ./ffplayout.json
ffplayout --db ./restored.db
```

The first command initializes the schema, validates the JSON format/version and
relationships, then imports all settings in one transaction. It exits without
starting playout or the web server. The dump format version controls compatibility
independently of database migration numbering.
`ffplayout_version` records the application version that created the export; it
does not restrict imports. `schema_version` only records the source migration
revision; renumbering migrations does not invalidate a structurally compatible
dump. Changes to settings structures still require explicit support in the importer.
Already configured databases are rejected,
even if they contain no users. No settings are merged with an existing installation.
Internal IDs are reassigned and their database references updated; channel IDs
and URLs containing those IDs may therefore change.

The new installation retains its new authentication signing key. Sessions and
refresh tokens are excluded, so users must log in again. A complete export of an
initialized installation restores its setup status, so initial setup is not
required again.

Exports contain settings and paths, **not files**: copy playlists, media, logos,
fonts and external scripts separately, and check paths and permissions on the
target machine. Runtime playlist position and time shift are not restored.

The JSON format replaces the former TOML channel dumps. `--import-config` accepts
only the versioned JSON format, with a maximum file size of 32 MiB. Old TOML
dumps are no longer supported; keep a database backup when upgrading from a
version that uses them. This format is independent of SQL statements, but
PostgreSQL and MariaDB backends are not implemented yet.

### macOS

The macOS desktop release does not include FFmpeg shared libraries. Install
FFmpeg separately with [Homebrew](https://brew.sh/):

```bash
brew install ffmpeg
```

Download and extract the desktop archive for your Mac from the
[ffplayout release page](https://github.com/ffplayout/ffplayout/releases/latest):
use `aarch64-apple-darwin` for Apple Silicon or `x86_64-apple-darwin` for an
Intel Mac. Start ffplayout from the extracted directory:

```bash
./ffplayout
```

The Homebrew FFmpeg package is separate software and is not part of the
ffplayout macOS release. Review the
[Homebrew formula information](https://formulae.brew.sh/formula/ffmpeg) and
[FFmpeg licensing information](https://ffmpeg.org/legal.html).

### Windows

The Windows release does not include FFmpeg DLLs. Install them separately:

1. Download and extract the Windows desktop archive from the [ffplayout release page](https://github.com/ffplayout/ffplayout/releases/latest).
2. Download the [BtbN FFmpeg 9.0 GPL shared build](https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip).
3. Extract the FFmpeg archive and copy all `*.dll` files from its `bin` directory into the ffplayout `bin` directory next to `ffplayout.exe`. Alternatively, add the extracted FFmpeg `bin` directory to the system `PATH`.
4. Start `bin\ffplayout.exe`.

FFmpeg is separate software and is not part of the ffplayout Windows release.
The selected BtbN build is GPL-enabled; review its included `LICENSE.txt` and
the [FFmpeg licensing information](https://ffmpeg.org/legal.html).

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
