## Build ffplayout

For compiling use always the news Rust version, the best is to install it from [rustup](https://rustup.rs/).

### Static Linking

Running `cargo build` ends up in a binary which depend on **libc.so**. But you can compile also the binary totally static:

- install musl compiler:
    - `dnf install musl-gcc`
- add target:
    - `rustup target add x86_64-unknown-linux-musl`

Compile with: `cargo build --release --target=x86_64-unknown-linux-musl`.

This release should run on any Linux distro.

**Note: You can also create a static version with Cross Toolchain. For this, follow the next steps.**

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

Start the development server on http://localhost:3000

```bash
npm run dev
```

## Production

Build the application for production:

```bash
npm run build
```

Locally preview production build:

```bash
npm run preview
```

Check out the [deployment documentation](https://nuxt.com/docs/getting-started/deployment) for more information.

### Experimental Frontend Features

To use experimental frontend features, add `NUXT_BUILD_EXPERIMENTAL=true` tu run and build command, like:

```
NUXT_BUILD_EXPERIMENTAL=true npm run dev
```

**Note:** This function is only for developers and testers who can do without support.
