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

Add toolchain:

```Bash
# for arm64
rustup target add aarch64-apple-darwin

# for x86_64
rustup target add x86_64-apple-darwin
```

Add linker and ar settings to `~/.cargo/config`:

```Bash
[target.x86_64-apple-darwin]
linker = "x86_64-apple-darwin20.4-clang"
ar = "x86_64-apple-darwin20.4-ar"

[target.aarch64-apple-darwin]
linker = "aarch64-apple-darwin20.4-clang"
ar = "aarch64-apple-darwin20.4-ar"
```

Follow this guide: [rust-cross-compile-linux-to-macos](https://wapl.es/rust/2019/02/17/rust-cross-compile-linux-to-macos.html)

Or setup [osxcross](https://github.com/tpoechtrager/osxcross) correctly.

Add **osxcross/target/bin** to your **PATH** and run cargo with:

```Bash
# for arm64
CC="aarch64-apple-darwin20.4-clang -arch arm64e" cargo build --release --target=aarch64-apple-darwin

# for x86_64
CC="o64-clang" cargo build --release --target=x86_64-apple-darwin
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
