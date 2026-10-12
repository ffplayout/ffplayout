FROM debian:trixie AS builder

ENV DEBIAN_FRONTEND=noninteractive \
    LOCALDESTDIR=/tmp/local \
    PKG_CONFIG="pkg-config --static" \
    PKG_CONFIG_PATH=/tmp/local/lib/pkgconfig:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    PKG_CONFIG_LIBDIR=/tmp/local/lib/pkgconfig:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    PKG_CONFIG_ALL_STATIC=1 \
    PKG_CONFIG_PREFER_STATIC=1 \
    CPPFLAGS="-I/tmp/local/include -fPIC" \
    CFLAGS="-I/tmp/local/include -mtune=generic -O2 -fPIC" \
    CXXFLAGS="-I/tmp/local/include -mtune=generic -O2 -fPIC" \
    LDFLAGS="-L/tmp/local/lib -pipe -static-libstdc++ -static-libgcc" \
    CC=gcc \
    CXX=g++

ARG FFMPEG_VAAPI=0

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        autoconf \
        automake \
        bzip2 \
        build-essential \
        ca-certificates \
        cmake \
        curl \
        git \
        gperf \
        libtool \
        meson \
        nasm \
        ninja-build \
        perl \
        pkg-config && \
    if [ "$FFMPEG_VAAPI" = 1 ]; then \
        apt-get install -y --no-install-recommends libva-dev libdrm-dev; \
    fi && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /tmp

RUN curl --retry 20 --retry-max-time 5 -L -f -o "zlib-1.3.2.tar.gz" "https://zlib.net/zlib-1.3.2.tar.gz" && \
    tar xf "zlib-1.3.2.tar.gz" && \
    cd "zlib-1.3.2" && \
    ./configure --prefix="$LOCALDESTDIR" --static && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -f -o "bzip2-1.0.8.tar.gz" "https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz" && \
    tar xf "bzip2-1.0.8.tar.gz" && \
    cd "bzip2-1.0.8" && \
    make -j "$(nproc)" && \
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

RUN curl --retry 20 --retry-max-time 5 -L -k -f -o "opus-1.6.tar.gz" "https://ftp.osuosl.org/pub/xiph/releases/opus/opus-1.6.tar.gz" && \
    tar xf "opus-1.6.tar.gz" && \
    cd "opus-1.6" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no --enable-static --disable-doc && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -o "openssl-3.5.0.tar.gz" "https://github.com/openssl/openssl/releases/download/openssl-3.5.0/openssl-3.5.0.tar.gz" && \
    tar xf "openssl-3.5.0.tar.gz" && \
    cd "openssl-3.5.0" && \
    ./Configure linux-x86_64 --prefix="$LOCALDESTDIR" --openssldir="$LOCALDESTDIR" --libdir=lib no-apps no-shared no-docs no-tests zlib -static -mtune=generic && \
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
    cmake ../source -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DCMAKE_CXX_FLAGS_RELEASE:STRING="-O3 -DNDEBUG $CXXFLAGS" && \
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

RUN git clone https://github.com/intel/libvpl.git && \
    cd libvpl && \
    cmake -S . -B build \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" \
        -DCMAKE_INSTALL_LIBDIR=lib \
        -DBUILD_SHARED_LIBS=OFF \
        -DBUILD_EXAMPLES=OFF \
        -DBUILD_TESTS=OFF && \
    cmake --build build -j"$(nproc)" && \
    cmake --install build && \
    sed -i '/^Libs.private:/ s/$/ -lstdc++/' "$LOCALDESTDIR/lib/pkgconfig/vpl.pc" && \
    pkg-config --modversion vpl

ARG FFMPEG_VERSION=release/9.0
ARG FFMPEG_AVDEVICE=0
ARG FFMPEG_AVFILTER=0

RUN git clone --depth 1 --branch "$FFMPEG_VERSION" https://github.com/FFmpeg/FFmpeg.git && cd FFmpeg && \
    avdevice_flag=--disable-avdevice && \
    avfilter_flag=--disable-avfilter && \
    vaapi_flags= && \
    if [ "$FFMPEG_AVDEVICE" = 1 ]; then avdevice_flag=--enable-avdevice; fi && \
    if [ "$FFMPEG_AVFILTER" = 1 ]; then avfilter_flag=--enable-avfilter; fi && \
    if [ "$FFMPEG_VAAPI" = 1 ]; then vaapi_flags="--enable-vaapi --enable-libdrm"; fi && \
    ./configure \
        --pkg-config-flags=--static \
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
        --enable-libvpl \
        $vaapi_flags \
        --enable-libsvtav1 \
        --enable-libdav1d && \
    make -j "$(nproc)" && \
    make install

FROM builder AS static-builder

ARG CARGO_FEATURES=embed_frontend
ARG FFMPEG_VAAPI=0

ENV DEBIAN_FRONTEND=noninteractive \
    FFPLAYOUT_VAAPI_SHARED=$FFMPEG_VAAPI \
    PKG_CONFIG=/usr/bin/pkg-config \
    PKG_CONFIG_ALL_STATIC=1 \
    PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    FFMPEG_PKG_CONFIG_PATH=/usr/local/lib/pkgconfig \
    LIBCLANG_PATH=/usr/lib/llvm-19/lib \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH \
    CARGO_FEATURES="$CARGO_FEATURES"

WORKDIR /src

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        clang \
        curl \
        gnupg \
        libclang-dev \
        libasound2-dev \
        liblzma-dev \
        libsqlite3-dev \
        perl \
        pkg-config \
        xz-utils && \
    curl -fsSL https://deb.nodesource.com/setup_24.x | bash - && \
    apt-get install -y --no-install-recommends nodejs && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
        sh -s -- -y --profile minimal --default-toolchain stable && \
    command -v pkg-config && \
    pkg-config --version && \
    node --version && \
    npm --version && \
    cargo install cargo-deb && \
    rm -rf /var/lib/apt/lists/*

CMD ["sh", "-c", "set -eux && echo 'Install frontend dependencies' && npm ci && echo 'Build frontend' && npm run build-only && echo 'Refresh FFmpeg link metadata' && cargo clean -p ffmpeg-sys-next --release && echo 'Build ffplayout binary' && cargo build --release --package ffplayout --no-default-features --features \"$CARGO_FEATURES\" && version=\"$(sed -n 's/^version = \"\\(.*\\)\"/\\1/p' Cargo.toml | head -1)\" && echo 'Copy build artifacts' && mkdir -p /artifacts && cp target/release/ffplayout /artifacts/ffplayout && echo 'Build deb package' && cargo deb --no-build -p ffplayout --manifest-path crates/app/Cargo.toml -o \"/artifacts/ffplayout_${version}-1_amd64.deb\" && echo 'Artifacts written to /artifacts'"]
