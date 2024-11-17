FROM alpine:latest as builder

ENV EXTRA_CFLAGS=-march=generic \
    LOCALBUILDDIR=/tmp/build \
    LOCALDESTDIR=/tmp/local \
    PKG_CONFIG="pkg-config --static" \
    PKG_CONFIG_PATH=/tmp/local/lib/pkgconfig \
    CPPFLAGS="-I/tmp/local/include -O3 -fno-strict-overflow -fstack-protector-all -fPIC" \
    CFLAGS="-I/tmp/local/include -O3 -fno-strict-overflow -fstack-protector-all -fPIC" \
    CXXFLAGS="-I/tmp/local/include -O2 -fPIC" \
    LDFLAGS="-L/tmp/local/lib -pipe -Wl,-z,relro,-z,now -static" \
    CC=clang

RUN apk add --no-cache \
    clang \
    glib-dev glib-static \
    coreutils \
    autoconf \
    automake \
    build-base \
    cmake \
    git \
    libtool \
    nasm \
    pkgconfig \
    yasm \
    wget \
    curl \
    ninja-build \
    meson \
    cargo cargo-c \
    diffutils \
    bash

RUN apk add --no-cache \
    zlib-dev zlib-static \
    bzip2-dev bzip2-static \
    expat-dev expat-static \
    libxml2-dev libxml2-static \
    fontconfig-dev fontconfig-static \
    freetype freetype-dev freetype-static \
    fribidi-dev fribidi-static \
    harfbuzz-dev harfbuzz-static \
    graphite2-static \
    numactl-dev \
    brotli-dev brotli-static \
    soxr-dev soxr-static \
    libjpeg-turbo libjpeg-turbo-dev \
    libpng-dev libpng-static \
    xvidcore-dev xvidcore-static \
    libsodium-dev libsodium-static \
    zeromq-dev libzmq-static \
    openssl-dev openssl-libs-static

WORKDIR /tmp
RUN git clone --depth 1 "https://github.com/libass/libass.git" && cd libass && \
    ./autogen.sh && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no && \
    make -j $(nproc) && \
    make install

RUN git clone --depth 1 "https://github.com/mstorsjo/fdk-aac" && cd fdk-aac && \
    ./autogen.sh && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no && \
    make -j $(nproc) && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "lame-3.100.tar.gz" "https://downloads.sourceforge.net/project/lame/lame/3.100/lame-3.100.tar.gz" && \
    tar xf "lame-3.100.tar.gz" && \
    cd "lame-3.100" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-expopt=full --enable-shared=no && \
    make -j $(nproc) && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "opus-1.4.tar.gz" "https://ftp.osuosl.org/pub/xiph/releases/opus/opus-1.4.tar.gz" && \
    tar xf "opus-1.4.tar.gz" && \
    cd "opus-1.4" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no --enable-static --disable-doc && \
    make -j $(nproc) && \
    make install

RUN git clone --depth 1 "https://github.com/Haivision/srt.git" && cd srt && \
    mkdir build && \
    cd build  && \
    cmake .. -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DOPENSSL_USE_STATIC_LIBS=ON -DUSE_STATIC_LIBSTDCXX:BOOLEAN=ON -DENABLE_CXX11:BOOLEAN=ON -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j $(nproc) && \
    make install

RUN git clone "https://github.com/webmproject/libvpx.git" && cd libvpx && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared --enable-static --disable-unit-tests --disable-docs --enable-postproc --enable-vp9-postproc --enable-runtime-cpu-detect && \
    make -j $(nproc) && \
    make install

RUN git clone "https://code.videolan.org/videolan/x264" && cd x264 && \
    ./configure --prefix="$LOCALDESTDIR" --enable-static && \
    make -j $(nproc) && \
    make install

RUN git clone "https://bitbucket.org/multicoreware/x265_git.git" && cd x265_git/build && \
    cmake ../source -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DCMAKE_CXX_FLAGS_RELEASE:STRING="-O3 -DNDEBUG $CXXFLAGS" && \
    make -j $(nproc) && \
    make install

RUN git clone "https://github.com/xiph/rav1e.git" && cd rav1e && \
    RUSTFLAGS="-C target-feature=+crt-static" cargo cinstall --release --jobs $(nproc) --prefix=$LOCALDESTDIR --libdir=$LOCALDESTDIR/lib --includedir=$LOCALDESTDIR/include

RUN git clone --depth 1 "https://gitlab.com/AOMediaCodec/SVT-AV1.git" && cd SVT-AV1/Build && \
    cmake .. -G"Unix Makefiles" -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j $(nproc) && \
    make install

RUN git clone --depth 1 "https://code.videolan.org/videolan/dav1d.git" && cd dav1d && \
    mkdir build && cd build && \
    meson setup -Denable_tools=false -Denable_tests=false --default-library=static .. --prefix "$LOCALDESTDIR" --libdir="$LOCALDESTDIR/lib" && \
    ninja && \
    ninja install

RUN git clone --depth 1 https://git.ffmpeg.org/ffmpeg.git && cd ffmpeg && \
    sed -i 's/add_ldexeflags -fPIE -pie/add_ldexeflags -fPIE -static-pie/' configure && \
    ./configure \
    --pkg-config-flags=--static \
    --extra-cflags="-fopenmp -DZMG_STATIC" \
    --extra-ldflags="-fopenmp -Wl,--copy-dt-needed-entries -Wl,--allow-multiple-definition" \
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
    --enable-libass \
    --enable-fontconfig \
    --enable-libfdk-aac \
    --enable-libfribidi \
    --enable-libfreetype \
    --enable-libharfbuzz \
    --enable-libmp3lame \
    --enable-libopus \
    --enable-libsoxr \
    --enable-libsrt \
    --enable-libvpx \
    --enable-libx264 \
    --enable-libx265 \
    --enable-libzmq \
    --enable-nonfree \
    --enable-openssl \
    --enable-libsvtav1 \
    --enable-librav1e \
    --enable-libdav1d \
    --enable-libxvid && \
    make -j $(nproc) && \
    make install

RUN strip /usr/local/bin/ffmpeg /usr/local/bin/ffprobe
