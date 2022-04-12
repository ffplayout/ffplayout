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

### Create debian DEB and RHEL RPM packages

install:
- `cargo install cargo-deb`
- `cargo install cargo-generate-rpm`

And run with:

```Bash
# for debian based systems:
cargo deb --target=x86_64-unknown-linux-musl

# for rhel based systems:
cargo generate-rpm --target=x86_64-unknown-linux-musl
```
