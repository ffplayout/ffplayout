### Cross Compile

For cross compiling on fedora linux, you need to install some extra packages:

- mingw compiler:
```
dnf install mingw{32,64}-filesystem mingw{32,64}-binutils mingw{32,64}-gcc{,-c++} mingw{32,64}-crt mingw{32,64}-headers mingw{32,64}-pkg-config mingw32-nsis mingw{32,64}-hamlib mingw{32,64}-libpng mingw{32,64}-libusbx mingw{32,64}-portaudio mingw{32,64}-fltk mingw{32,64}-libgnurx mingw{32,64}-gettext mingw{32,64}-winpthreads-static intltool
```

- rust tools:
```
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup target add x86_64-pc-windows-gnu
```

[Cross](https://github.com/cross-rs/cross#dependencies) could be an option to.

To build, run: `cargo build --release --target=x86_64-pc-windows-gnu`
