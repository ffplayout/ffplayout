FROM debian:trixie AS builder

ENV DEBIAN_FRONTEND=noninteractive \
    LOCALDESTDIR=/tmp/local \
    PKG_CONFIG="pkg-config --static" \
    PKG_CONFIG_PATH=/tmp/local/lib/pkgconfig:/usr/local/lib/pkgconfig:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    CPPFLAGS="-I/tmp/local/include -O3 -fPIC" \
    CFLAGS="-I/tmp/local/include -O3 -fPIC" \
    CXXFLAGS="-I/tmp/local/include -O2 -fPIC" \
    LDFLAGS="-L/tmp/local/lib -pipe -static" \
    CC=gcc \
    CXX=g++

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        autoconf \
        automake \
        bash \
        binutils \
        build-essential \
        ca-certificates \
        clang \
        cmake \
        curl \
        diffutils \
        file \
        git \
        libbrotli-dev \
        libbz2-dev \
        libexpat1-dev \
        libfontconfig-dev \
        libfreetype-dev \
        libfribidi-dev \
        libglib2.0-dev \
        libgraphite2-dev \
        libjpeg-dev \
        liblzma-dev \
        libnuma-dev \
        libpng-dev \
        libsodium-dev \
        libtool \
        libxml2-dev \
        libzstd-dev \
        llvm \
        meson \
        nasm \
        ninja-build \
        pkg-config \
        wget \
        yasm \
        zlib1g-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /tmp

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
    ./Configure --prefix=$LOCALDESTDIR --openssldir=$LOCALDESTDIR $target --libdir="$LOCALDESTDIR/lib" no-shared no-docs zlib -static && \
    make depend all && \
    make install_sw

RUN git clone --depth 1 "https://github.com/Haivision/srt.git" && cd srt && \
    mkdir build && \
    cd build && \
    cmake .. -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DOPENSSL_USE_STATIC_LIBS=ON -DUSE_STATIC_LIBSTDCXX:BOOLEAN=ON -DENABLE_CXX11:BOOLEAN=ON -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j "$(nproc)" && \
    make install

RUN git clone "https://github.com/webmproject/libvpx.git" && cd libvpx && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared --enable-static --disable-unit-tests --disable-docs --enable-postproc --enable-vp9-postproc --enable-runtime-cpu-detect && \
    make -j "$(nproc)" && \
    make install

RUN git clone "https://code.videolan.org/videolan/x264" && cd x264 && \
    ./configure --prefix="$LOCALDESTDIR" --enable-static && \
    make -j "$(nproc)" && \
    make install

RUN git clone "https://bitbucket.org/multicoreware/x265_git.git" && cd x265_git/build && \
    cmake ../source -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DCMAKE_CXX_FLAGS_RELEASE:STRING="-O3 -DNDEBUG $CXXFLAGS" && \
    make -j "$(nproc)" && \
    make install

RUN git clone --depth 1 "https://gitlab.com/AOMediaCodec/SVT-AV1.git" && cd SVT-AV1/Build && \
    cmake .. -G"Unix Makefiles" -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
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

RUN git clone --depth 1 --branch v4.3.5 "https://github.com/zeromq/libzmq.git" && cd libzmq && \
    ./autogen.sh && \
    ./configure \
        --prefix="$LOCALDESTDIR" \
        --enable-static \
        --disable-shared \
        --with-libsodium \
        --without-pgm \
        --without-norm \
        --without-vmci \
        --without-docs && \
    make -j "$(nproc)" && \
    make install

ARG FFMPEG_VERSION=release/8.1
ARG FFMPEG_DEBUG=0

RUN mkdir -p /ffmpeg-debug && \
    git clone --depth 1 --branch "$FFMPEG_VERSION" https://github.com/FFmpeg/FFmpeg.git && cd FFmpeg && \
    if ! ./configure \
        --pkg-config-flags=--static \
        --extra-cflags="-DZMG_STATIC" \
        --extra-ldflags="-Wl,--copy-dt-needed-entries -Wl,--allow-multiple-definition" \
        --enable-runtime-cpudetect \
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
        --enable-libzmq \
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
