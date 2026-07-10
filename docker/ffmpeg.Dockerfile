FROM debian:trixie AS builder

ENV DEBIAN_FRONTEND=noninteractive \
    LOCALDESTDIR=/tmp/local \
    PKG_CONFIG="pkg-config --static" \
    PKG_CONFIG_PATH=/tmp/local/lib/pkgconfig \
    PKG_CONFIG_LIBDIR=/tmp/local/lib/pkgconfig \
    PKG_CONFIG_ALL_STATIC=1 \
    PKG_CONFIG_PREFER_STATIC=1 \
    CPPFLAGS="-I/tmp/local/include -fPIC" \
    CFLAGS="-I/tmp/local/include -mtune=generic -O2 -fPIC" \
    CXXFLAGS="-I/tmp/local/include -mtune=generic -O2 -fPIC" \
    LDFLAGS="-L/tmp/local/lib -pipe -static-libstdc++ -static-libgcc" \
    CC=gcc \
    CXX=g++

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
        pkg-config \
        libpulse-dev \
        libx11-dev \
        libxext-dev \
        libxrandr-dev \
        libxcursor-dev \
        libxfixes-dev \
        libxi-dev \
        libxss-dev \
        libxkbcommon-dev \
        libwayland-dev \
        libgbm-dev \
        libgl1-mesa-dev \
        libegl1-mesa-dev \
        libudev-dev \
        libdbus-1-dev \
        libpipewire-0.3-dev \
        xz-utils && \
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

RUN git clone --depth 1 "https://github.com/fribidi/fribidi.git" && cd fribidi && \
    meson setup build \
        --default-library=static \
        --prefix "$LOCALDESTDIR" \
        --libdir="$LOCALDESTDIR/lib" \
        -Ddocs=false \
        -Dbin=false \
        -Dtests=false && \
    ninja -C build && \
    ninja -C build install

RUN curl --retry 20 --retry-max-time 5 -L -f -o "expat-2.7.1.tar.bz2" "https://github.com/libexpat/libexpat/releases/download/R_2_7_1/expat-2.7.1.tar.bz2" && \
    tar xf "expat-2.7.1.tar.bz2" && \
    cd "expat-2.7.1" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no --without-docbook && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -f -o "brotli-1.1.0.tar.gz" "https://github.com/google/brotli/archive/refs/tags/v1.1.0.tar.gz" && \
    tar xf "brotli-1.1.0.tar.gz" && \
    cd "brotli-1.1.0" && \
    cmake -S . -B build \
        -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" \
        -DCMAKE_BUILD_TYPE=Release \
        -DBUILD_SHARED_LIBS=OFF \
        -DCMAKE_INSTALL_LIBDIR=lib && \
    cmake --build build -j "$(nproc)" && \
    cmake --install build

RUN curl --retry 20 --retry-max-time 5 -L -f -o "freetype-2.13.3.tar.gz" "https://sourceforge.net/projects/freetype/files/freetype2/2.13.3/freetype-2.13.3.tar.gz" && \
    tar xf "freetype-2.13.3.tar.gz" && \
    cd "freetype-2.13.3" && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared --with-harfbuzz=no && \
    make -j "$(nproc)" && \
    make install

RUN git clone --depth 1 --branch 2.16.0 "https://gitlab.freedesktop.org/fontconfig/fontconfig.git" && cd fontconfig && \
    meson setup build \
        --default-library=static \
        --prefix "$LOCALDESTDIR" \
        --libdir="$LOCALDESTDIR/lib" \
        -Dcache-build=disabled \
        -Ddoc=disabled \
        -Dnls=disabled \
        -Dtests=disabled \
        -Dtools=disabled \
        -Dxml-backend=expat && \
    ninja -C build && \
    ninja -C build install

RUN git clone --depth 1 "https://github.com/mstorsjo/fdk-aac" && cd fdk-aac && \
    ./autogen.sh && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no && \
    make -j "$(nproc)" && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -o "lame-3.100.tar.gz" "https://downloads.sourceforge.net/project/lame/lame/3.100/lame-3.100.tar.gz" && \
    tar xf "lame-3.100.tar.gz" && \
    cd "lame-3.100" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-expopt=full --enable-shared=no && \
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

RUN git clone --depth 1 --branch 10.2.0 "https://github.com/harfbuzz/harfbuzz.git" && cd harfbuzz && \
    meson setup build \
        --default-library=static \
        --prefix "$LOCALDESTDIR" \
        --libdir "$LOCALDESTDIR/lib" \
        -Dglib=disabled \
        -Dgobject=disabled \
        -Dicu=disabled \
        -Dcairo=disabled \
        -Dchafa=disabled \
        -Dtests=disabled \
        -Ddocs=disabled \
        -Dbenchmark=disabled && \
    ninja -C build && \
    ninja -C build install

RUN git clone --depth 1 https://github.com/alsa-project/alsa-lib && \
    cd alsa-lib && \
    autoreconf -i && \
    ./configure --enable-shared=no --enable-static=yes --without-libdl && \
    make -j "$(nproc)" && \
    make install

RUN git clone --depth 1 --branch SDL2 https://github.com/libsdl-org/SDL.git SDL2 && \
    cmake -S SDL2 -B SDL2/build \
        -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" \
        -DSDL_SHARED=OFF \
        -DSDL_STATIC=ON \
        -DSDL_TESTS=OFF \
        -DSDL_TEST_LIBRARY=OFF \
        -DSDL2_DISABLE_INSTALL=OFF && \
    cmake --build SDL2/build  && \
    cmake --install SDL2/build

ARG FFMPEG_VERSION=release/8.1
ARG FFMPEG_DEBUG=0

RUN mkdir -p /ffmpeg-debug && \
    git clone --depth 1 --branch "$FFMPEG_VERSION" https://github.com/FFmpeg/FFmpeg.git && cd FFmpeg && \
    if ! ./configure \
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
        --enable-avfilter \
        --enable-gpl \
        --enable-version3 \
        --enable-nonfree \
        --enable-static \
        --enable-fontconfig \
        --enable-libfdk-aac \
        --enable-libfribidi \
        --enable-libfreetype \
        --enable-libharfbuzz \
        --enable-libmp3lame \
        --enable-libopus \
        --enable-libsrt \
        --enable-libvpx \
        --enable-libx264 \
        --enable-libx265 \
        --enable-openssl \
        --enable-libsvtav1 \
        --enable-libdav1d; then \
        status=1; \
        cp -a /tmp/FFmpeg /ffmpeg-debug/ffmpeg; \
        { \
            echo "PKG_CONFIG=$PKG_CONFIG"; \
            echo "PKG_CONFIG_PATH=$PKG_CONFIG_PATH"; \
            echo "CFLAGS=$CFLAGS"; \
            echo "LDFLAGS=$LDFLAGS"; \
            find /tmp/local /usr/lib /usr/share -name '*harfbuzz*.pc' -print; \
            pkg-config --debug --print-errors --static --cflags --libs harfbuzz; \
        } > /ffmpeg-debug/harfbuzz-pkg-config.log 2>&1 || true; \
        if [ "$FFMPEG_DEBUG" = "1" ]; then exit 0; fi; \
        exit "$status"; \
    fi && \
    make -j "$(nproc)" && \
    make install

FROM scratch AS ffmpeg-debug

COPY --from=builder /ffmpeg-debug/ /

FROM builder AS ffmpeg-static

RUN strip /usr/local/bin/ffmpeg /usr/local/bin/ffprobe
