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
    libavdevice-dev \
    libavfilter-dev \
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
[BtbN FFmpeg 8.1 GPL shared build](https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip).
Point `ffmpeg-sys-next` to the extracted directory and make the DLLs available
while compiling:

```powershell
$env:FFMPEG_DIR = "C:\path\to\ffmpeg-n8.1-latest-win64-gpl-shared-8.1"
$env:PATH = "$env:FFMPEG_DIR\bin;$env:PATH"
cargo build
```

When compiling against a manually installed FFmpeg, make sure `pkg-config` can find the `.pc` files, for example:

```bash
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH
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
from `backend/app`:

```bash
cargo build --release -p ffplayout
cd backend/app
cargo generate-rpm
```

For cross-compiled packages, provide a complete, compatible target FFmpeg
environment and verify the resulting binary on the target system.

## Generate types for Frontend
The frontend uses TypeScript, to generate types for the rust structs run: `cargo test`.

The generated types are written next to the frontend sources that import them.

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
