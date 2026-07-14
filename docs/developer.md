## Build ffplayout

For compiling, use the latest stable Rust version from [rustup](https://rustup.rs/).

### FFmpeg libraries

ffplayout links against FFmpeg through `ffmpeg-next`, so local builds need the FFmpeg runtime tools and development libraries. FFmpeg 7.0+ is required.

On Debian/Ubuntu based systems install:

```bash
sudo apt install \
    ffmpeg \
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

When compiling against a manually installed FFmpeg, make sure `pkg-config` can find the `.pc` files, for example:

```bash
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH
```

### Create debian DEB and RHEL RPM packages

install:
- `cargo install cargo-deb`
- `cargo install cargo-generate-rpm`

Compile to your target system with cargo, and run:

```Bash
# for debian based systems:
cargo deb --no-build -p ffplayout

# for armhf
cargo deb --no-build --target=armv7-unknown-linux-gnueabihf --variant=armhf -p ffplayout

# for arm64
cargo deb --no-build --target=aarch64-unknown-linux-gnu --variant=arm64 -p ffplayout

# for rhel based systems:
cargo generate-rpm --target=x86_64-unknown-linux-musl
```

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
