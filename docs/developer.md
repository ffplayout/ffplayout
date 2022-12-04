## Build ffplayout

For compiling use always the news Rust version, the best is to install it from [rustup](https://rustup.rs/).

### Cross Compile

For cross compiling on fedora linux, you need to install some extra packages:

- mingw compiler:
```
dnf install mingw64-filesystem mingw64-binutils mingw64-gcc{,-c++} mingw64-crt mingw64-headers mingw64-pkg-config mingw64-hamlib mingw64-libpng mingw64-libusbx mingw64-portaudio mingw64-fltk mingw64-libgnurx mingw64-gettext mingw64-winpthreads-static intltool
```

- rust tools:
```
rustup target add x86_64-pc-windows-gnu
```

[Cross](https://github.com/cross-rs/cross#dependencies) could be an option to.

To build, run: `cargo build --release --target=x86_64-pc-windows-gnu`

### Static Linking

Running `cargo build` ends up in a binary which depend on **libc.so**. But you can compile also the binary totally static:

- install musl compiler:
    - `dnf install musl-gcc`
- add target:
    - `rustup target add x86_64-unknown-linux-musl`

Compile with: `cargo build --release --target=x86_64-unknown-linux-musl`.

This release should run on any Linux distro.

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

### Compile for armv7 Linux

Add toolchain:

```Bash
rustup target add armv7-unknown-linux-gnueabihf
```

Add cross compiler:

```Bash
dnf copr enable lantw44/arm-linux-gnueabihf-toolchain

dnf install arm-linux-gnueabihf-{binutils,gcc,glibc}
```

Add target to `~/.cargo/config`:

```Bash
[target.armv7-unknown-linux-gnueabihf]
linker = "arm-linux-gnueabihf-gcc"
rustflags = [ "-C", "target-feature=+crt-static", "-C", "link-arg=-lgcc" ]
```

### Compile for aarch64 Linux

Add toolchain:

```Bash
rustup target add aarch64-unknown-linux-gnu
```

Add cross compiler:

```Bash
dnf copr enable lantw44/aarch64-linux-gnu-toolchain

dnf install aarch64-linux-gnu-{binutils,gcc,glibc}
```

Add target to `~/.cargo/config`:

```Bash
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
rustflags = [ "-C", "target-feature=+crt-static", "-C", "link-arg=-lgcc" ]
```

### Create debian DEB and RHEL RPM packages

install:
- `cargo install cargo-deb`
- `cargo install cargo-generate-rpm`

And run with:

```Bash
# for debian based systems:
cargo deb --target=x86_64-unknown-linux-musl

# for armhf
cargo deb --target=armv7-unknown-linux-gnueabihf --variant=armhf -p ffplayout --manifest-path=ffplayout-engine/Cargo.toml

# for arm64
cargo deb --target=aarch64-unknown-linux-gnu --variant=arm64 -p ffplayout --manifest-path=ffplayout-engine/Cargo.toml

# for rhel based systems:
cargo generate-rpm --target=x86_64-unknown-linux-musl
```
