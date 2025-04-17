## Build ffplayout

For compiling use always the news Rust version, the best is to install it from [rustup](https://rustup.rs/).

### Cross Compile

For cross compiling install docker or podman and latest [cross-rs](https://github.com/cross-rs/cross):

```
cargo install cross --git https://github.com/cross-rs/cross
```

To build for windows, run: `cross build --release --target x86_64-pc-windows-gnu`\
To build for linux aarch64: `cross build --release --target aarch64-unknown-linux-gnu`
Etc.

### Compile from Linux for macOS

Follow [cross-toolchains](https://github.com/cross-rs/cross-toolchains) instruction to add macOS support to cross.

I created my image with:

```
cargo build-docker-image x86_64-apple-darwin-cross \
    --build-arg 'MACOS_SDK_URL=https://github.com/joseluisq/macosx-sdks/releases/download/12.3/MacOSX12.3.sdk.tar.xz'
```

Build then with:

```
cross build --release --target aarch64-apple-darwin
```

```
### Create debian DEB and RHEL RPM packages

install:
- `cargo install cargo-deb`
- `cargo install cargo-generate-rpm`

Compile to your target system with cargo or cross, and run:

```Bash
# for debian based systems:
cargo deb --no-build --target=x86_64-unknown-linux-musl

# for armhf
cargo deb --no-build --target=armv7-unknown-linux-gnueabihf --variant=armhf -p ffplayout --manifest-path=ffplayout-engine/Cargo.toml

# for arm64
cargo deb --no-build --target=aarch64-unknown-linux-gnu --variant=arm64 -p ffplayout --manifest-path=ffplayout-engine/Cargo.toml

# for rhel based systems:
cargo generate-rpm --target=x86_64-unknown-linux-musl
```

## Generate types for Frontend
The frontend uses TypeScript, to generate types for the rust structs run: `cargo test`.

The generated types are then in [types folder](/frontend/types).

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

1. initialize database: `cargo run -- -i`
2. run backend: `cargo run -- -l 127.0.0.1:8787`
3. in second terminal:
    1. install packages: `npm i`
    2. run frontend: `npm run dev`
4. in browser navigate to: `127.0.0.1:5757`
