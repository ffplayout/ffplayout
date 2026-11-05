# Native musl build, intentionally limited to software/server outputs on x86_64.
FROM rust:1-alpine3.23 AS toolchain

RUN apk add --no-cache \
        autoconf automake bash binutils build-base bzip2 ca-certificates \
        clang clang-dev cmake curl diffutils git gperf libtool linux-headers \
        meson nasm ninja nodejs npm perl pkgconf python3 xz && \
    test "$(uname -m)" = x86_64 && \
    test -f "$(g++ -print-file-name=libstdc++.a)" && \
    rustc --version && node --version && npm --version

FROM toolchain AS builder

ARG FFMPEG_VAAPI=0
RUN if [ "$FFMPEG_VAAPI" != 0 ]; then \
        echo 'The fully static musl builder does not support VAAPI or Intel VPL.' >&2; \
        exit 1; \
    fi

ENV LOCALDESTDIR=/tmp/local \
    PKG_CONFIG="pkg-config --static" \
    PKG_CONFIG_PATH=/tmp/local/lib/pkgconfig \
    PKG_CONFIG_LIBDIR=/tmp/local/lib/pkgconfig \
    PKG_CONFIG_ALL_STATIC=1 \
    PKG_CONFIG_PREFER_STATIC=1 \
    CPPFLAGS="-I/tmp/local/include -fPIC" \
    CFLAGS="-I/tmp/local/include -mtune=generic -O2 -fPIC" \
    CXXFLAGS="-I/tmp/local/include -mtune=generic -O2 -fPIC" \
    LDFLAGS="-L/tmp/local/lib -static -static-libstdc++ -static-libgcc" \
    CC=gcc \
    CXX=g++

WORKDIR /tmp

RUN curl --retry 20 --retry-max-time 5 -L -f -o "zlib-1.4.tar.gz" "https://zlib.net/zlib-1.4.tar.gz" && \
    tar xf "zlib-1.4.tar.gz" && \
    cd "zlib-1.4" && \
    ./configure --prefix="$LOCALDESTDIR" --static && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -f -o "bzip2-1.0.8.tar.gz" "https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz" && \
    tar xf "bzip2-1.0.8.tar.gz" && \
    cd "bzip2-1.0.8" && \
    make -j "$(nproc)" CFLAGS="$CFLAGS" && \
    make install PREFIX="$LOCALDESTDIR"

RUN curl --retry 20 --retry-max-time 5 -L -f -o "xz-5.4.3.tar.gz" "https://downloads.sourceforge.net/project/lzmautils/xz-5.4.3.tar.gz" && \
    tar xf "xz-5.4.3.tar.gz" && \
    cd "xz-5.4.3" && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -f -o "libpng-1.6.48.tar.gz" "https://download.sourceforge.net/libpng/libpng-1.6.48.tar.gz" && \
    tar xf "libpng-1.6.48.tar.gz" && \
    cd "libpng-1.6.48" && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared && \
    make -j "$(nproc)" && \
    make install

RUN git clone --depth 1 "https://github.com/mstorsjo/fdk-aac" && cd fdk-aac && \
    ./autogen.sh && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -f -o "opus-1.6.tar.gz" "https://ftp.osuosl.org/pub/xiph/releases/opus/opus-1.6.tar.gz" && \
    tar xf "opus-1.6.tar.gz" && \
    cd "opus-1.6" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no --enable-static --disable-doc && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -f -o "openssl-3.5.0.tar.gz" "https://github.com/openssl/openssl/releases/download/openssl-3.5.0/openssl-3.5.0.tar.gz" && \
    tar xf "openssl-3.5.0.tar.gz" && \
    cd "openssl-3.5.0" && \
    ./Configure linux-x86_64 --prefix="$LOCALDESTDIR" --openssldir="$LOCALDESTDIR" --libdir=lib no-apps no-shared no-docs no-tests no-async zlib -static -mtune=generic && \
    make -j "$(nproc)" build_sw && \
    make install_sw

RUN git clone --depth 1 "https://github.com/Haivision/srt.git" && cd srt && \
    mkdir build && \
    cd build && \
    cmake .. -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DOPENSSL_USE_STATIC_LIBS=ON -DUSE_STATIC_LIBSTDCXX:BOOLEAN=ON -DENABLE_CXX11:BOOLEAN=OFF -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j "$(nproc)" && \
    make install && \
    sed -i '/^Libs:/ s/$/ -lstdc++ -lcrypto -lz -lpthread -ldl/' "$LOCALDESTDIR/lib/pkgconfig/srt.pc"

RUN git clone "https://github.com/webmproject/libvpx.git" && cd libvpx && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared --enable-static --enable-pic --disable-unit-tests --disable-docs --enable-postproc --enable-vp9-postproc --enable-runtime-cpu-detect && \
    make -j "$(nproc)" && \
    make install

RUN git clone "https://code.videolan.org/videolan/x264" && cd x264 && \
    ./configure --prefix="$LOCALDESTDIR" --enable-static --enable-pic && \
    make -j "$(nproc)" && \
    make install

RUN git clone "https://bitbucket.org/multicoreware/x265_git.git" && cd x265_git/build && \
    cmake ../source -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DENABLE_LIBNUMA=OFF -DCMAKE_CXX_FLAGS_RELEASE:STRING="-O3 -DNDEBUG $CXXFLAGS" && \
    make -j "$(nproc)" && \
    make install && \
    sed -ri "s/(Libs\:.*)/\1 -lstdc++ -lpthread -ldl/g" "$LOCALDESTDIR/lib/pkgconfig/x265.pc"

RUN git clone --depth 1 "https://gitlab.com/AOMediaCodec/SVT-AV1.git" && cd SVT-AV1/Build && \
    cmake .. -G"Unix Makefiles" -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DSVT_AV1_LTO=OFF -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j "$(nproc)" && \
    make install

RUN git clone --depth 1 "https://code.videolan.org/videolan/dav1d.git" && cd dav1d && \
    mkdir build && cd build && \
    meson setup -Denable_tools=false -Denable_tests=false --default-library=static .. --prefix "$LOCALDESTDIR" --libdir="$LOCALDESTDIR/lib" && \
    ninja && \
    ninja install

ARG FFMPEG_VERSION=release/9.0
ARG FFMPEG_AVDEVICE=0
ARG FFMPEG_AVFILTER=0

RUN git clone --depth 1 --branch "$FFMPEG_VERSION" https://github.com/FFmpeg/FFmpeg.git && cd FFmpeg && \
    avdevice_flag=--disable-avdevice && \
    avfilter_flag=--disable-avfilter && \
    if [ "$FFMPEG_AVDEVICE" = 1 ]; then avdevice_flag=--enable-avdevice; fi && \
    if [ "$FFMPEG_AVFILTER" = 1 ]; then avfilter_flag=--enable-avfilter; fi && \
    ./configure \
        --pkg-config-flags=--static \
        --disable-autodetect \
        --extra-ldflags="-static" \
        --extra-libs="-lm -lpthread" \
        --enable-runtime-cpudetect \
        --enable-pic \
        --enable-bzlib \
        --enable-lzma \
        --enable-zlib \
        --prefix=/usr/local \
        --disable-debug \
        --disable-doc \
        --disable-ffplay \
        --disable-shared \
        "$avdevice_flag" \
        "$avfilter_flag" \
        --enable-gpl \
        --enable-version3 \
        --enable-nonfree \
        --enable-static \
        --enable-libfdk-aac \
        --enable-libopus \
        --enable-libsrt \
        --enable-libvpx \
        --enable-libx264 \
        --enable-libx265 \
        --enable-openssl \
        --enable-libsvtav1 \
        --enable-libdav1d && \
    make -j "$(nproc)" && \
    make install

FROM builder AS static-builder

ARG CARGO_FEATURES=embed_frontend

ENV PKG_CONFIG=/usr/bin/pkg-config \
    PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:/tmp/local/lib/pkgconfig \
    PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig:/tmp/local/lib/pkgconfig \
    FFMPEG_PKG_CONFIG_PATH=/usr/local/lib/pkgconfig \
    CARGO_BUILD_TARGET=x86_64-unknown-linux-musl \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=gcc \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-feature=+crt-static" \
    CARGO_TARGET_DIR=/src/target/alpine-musl-host-dynamic \
    CARGO_FEATURES="$CARGO_FEATURES"

WORKDIR /src
COPY scripts/build-musl.sh /usr/local/bin/build-ffplayout-musl
COPY --chmod=755 scripts/rustc-musl-host.sh /usr/local/bin/rustc-musl-host

CMD ["sh", "-c", "exec sh /usr/local/bin/build-ffplayout-musl"]
