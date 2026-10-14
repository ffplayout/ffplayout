## Build ffplayout

For compiling, use the latest stable Rust version from [rustup](https://rustup.rs/).

### FFmpeg libraries

ffplayout links against FFmpeg through `ffmpeg-next`, so local builds need the FFmpeg runtime tools and development libraries. FFmpeg 7.0+ is required.

On Debian/Ubuntu based systems install:

```bash
sudo apt install \
    ffmpeg \
    libasound2-dev \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libswresample-dev \
    libswscale-dev \
    pkg-config
```

On macOS install:

```bash
brew install ffmpeg pkg-config
```

On Windows, download and extract the
[BtbN FFmpeg 9.0 GPL shared build](https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip).
Point `ffmpeg-sys-next` to the extracted directory and make the DLLs available
while compiling:

```powershell
$env:FFMPEG_DIR = "C:\path\to\ffmpeg-n9.0-latest-win64-gpl-shared-9.0"
$env:PATH = "$env:FFMPEG_DIR\bin;$env:PATH"
cargo build
```

When compiling against a manually installed FFmpeg, make sure `pkg-config` can find the `.pc` files, for example:

```bash
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH
```

### Optional SRT logging integration

The engine's build script uses `pkg-config` to check whether libsrt is linkable
for the target build. If `pkg-config --modversion srt` succeeds, ffplayout links
libsrt directly and routes its native messages through the ffplayout logger,
including rate limiting for repeated receive-buffer warnings. Use the
development files for the same libsrt installation as your FFmpeg build.

If libsrt or its `srt.pc` file is unavailable, ffplayout still builds and SRT
ingest can still work through an FFmpeg build that supports it. In that case,
libsrt's native messages may appear directly on stderr instead of in the
ffplayout logs. The provided static Docker build installs `srt.pc` alongside
libsrt, so its logger integration is enabled automatically.

`libavfilter` and `libavdevice` are optional and disabled by default to reduce
the runtime and static-build footprint. ffplayout currently uses its own media
processing pipeline, so FFmpeg filters such as `drawtext` and FFmpeg devices
such as DeckLink are unavailable in the normal build.

Enable them independently with the `ffmpeg-filter` or `ffmpeg-device` Cargo
feature. Local builds additionally need `libavfilter-dev` or `libavdevice-dev`.
The static-build script maps those features to FFmpeg automatically, for
example:

```bash
CARGO_FEATURES=desktop,embed_frontend,ffmpeg-filter,ffmpeg-device \\
    ./scripts/build.sh debian-static
```

## Processing Benchmark

The optional `processing-bench` feature prints periodic wall-clock timings for
video and audio decode, audio processing and encoding, scale, overlay,
subtitle, encoded-output, and desktop presentation stages. It is intended for local CPU
profiling and is disabled in normal builds.

```bash
cargo run -p ffplayout --features processing-bench -- \
    --processing-bench-interval 5 \
    -l 127.0.0.1:8787 --log-to-console -o hls
```

`--processing-bench-interval` is measured in seconds and defaults to `1`.
The `FFPLAYOUT_PROCESSING_BENCH_INTERVAL` environment variable provides the
same setting. The reported times measure the instrumented caller thread; use
`perf` for CPU time inside FFmpeg worker threads.

## Hardware Encoding

The output codec list includes hardware encoders only when they accept the
engine's CPU-backed frames. NVENC accepts `yuv420p` directly. QSV receives a
CPU-side `nv12` frame produced immediately before the encoder and performs the
surface upload internally. VAAPI keeps decoding, scaling and overlays on the
CPU, then uploads the completed `nv12` frame to `/dev/dri/renderD128`
immediately before encoding. The process therefore needs access to the render
device, normally through the `render` group, in addition to a matching GPU
driver.

### Create Debian DEB and RHEL RPM Packages

Install the packaging tools:

- `cargo install cargo-deb`
- `cargo install cargo-generate-rpm`

Build packages on the target distribution and architecture. `ffmpeg-next` links
against the system FFmpeg libraries, so cross-compilation also requires a target
sysroot containing matching FFmpeg development libraries and `pkg-config`
metadata. Rust's `--target` option alone is not sufficient.

For Debian-based systems:

```bash
cargo build --release -p ffplayout
cargo deb --no-build -p ffplayout
```

For RHEL-based systems, build the release binary first, then generate the RPM
from `crates/app`:

```bash
cargo build --release -p ffplayout
cd crates/app
cargo generate-rpm
```

For cross-compiled packages, provide a complete, compatible target FFmpeg
environment and verify the resulting binary on the target system.

## Generate types for Frontend
The frontend uses TypeScript, to generate types for the rust structs run: `cargo test`.

The generated types are written next to the frontend sources that import them.

## Database migrations on `main`

Database migrations added on the `main` branch are considered unstable until
the next ffplayout version containing them is released. Before that release,
an existing migration may still be changed, reordered, or combined with
another migration.

Running a self-built development version can therefore apply an intermediate
migration that is incompatible with a later commit. This can break the
migration chain or cause a migration checksum mismatch, and the affected
database may have to be reinitialized. Do not use builds from `main` with a
production database without first creating a restorable backup. Once a
migration has been included in a release, it must remain unchanged; subsequent
schema changes require a new migration.

## Setup Frontend

Make sure to install the dependencies:

```bash
# yarn
yarn install

# npm
npm install

# pnpm
pnpm install --shamefully-hoist
```

## Development Server

Start the development server on http://127.0.0.1:5757

```bash
npm run dev
```

## Production

Build the application for production:

```bash
npm run build
```

Check out the [deployment documentation](https://vuejs.org/guide/quick-start.html) for more information.

## Run ffplayout in development mode

1. run backend: `cargo run -- -l 127.0.0.1:8787`
2. in a second terminal:
    1. install packages: `npm ci`
    2. run frontend: `npm run dev`
3. in the browser navigate to: `http://127.0.0.1:5757`
4. Complete first-time setup in the frontend.

For an initial setup without the frontend, use `cargo run -- -i` instead.

### Memory diagnostics

Use the hidden startup flag together with DEBUG logging:

```sh
ffplayout --memory-diagnostics --log-level DEBUG
```

Alternatively, use the Clap environment variable:

```sh
FFPLAYOUT_MEMORY_DIAGNOSTICS=true ffplayout --log-level DEBUG
```

For systemd, add `Environment="FFPLAYOUT_MEMORY_DIAGNOSTICS=true"` to the service
configuration and restart the service. Both forms enable diagnostics for all
running channels. The setting defaults to false; an explicit CLI flag enables
it even if the environment variable is false. This setting is not part of the
TOML configuration or database and has no frontend field.

`[Memory Diagnostics]` DEBUG records are produced immediately before opening each
non-skipped clip (after the preceding playback call has returned), and on actual
live takeover/end events. The first clip establishes the RSS baseline. Clip numbers
count playback attempts, including filler, repeated sources and post-live attempts.
A single worker serializes measurements and logging; queued live transitions are
processed before the next clip boundary. Playback waits for its clip record.
Each live event requests a measurement asynchronously; this is not a snapshot
captured inside the decoder at the exact instant of the transition.

Records contain `scope=process`, event, channel metadata, clip number, process
uptime in seconds, RSS bytes, signed RSS delta since the previous measurement for
that channel, PSS bytes on Linux, thread count where supported, live status,
listener ID and source kind (`playlist`, `folder`, `filler`, or `ingest`).
No media file paths are included in these diagnostic records. Live start uses
`ingest`; live end uses the preceding program source kind until the next clip
boundary establishes the newly selected source. Unavailable measurements are marked `unavailable`.
Linux PSS is read from `/proc/self/smaps_rollup`. RSS/PSS cover the entire process,
so measurements from different channels cannot attribute growth to one channel.
The live end retains the listener ID of the ended session.

The feature has no periodic sampling timer. Process inspection runs outside the
async executor on blocking workers, and no measurements run unless diagnostics
are enabled and DEBUG logging is active. Existing periodic development metrics
remain independent.
