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
