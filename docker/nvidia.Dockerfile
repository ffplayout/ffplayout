FROM nvidia/cuda:12.5.0-runtime-rockylinux9

ARG FFPLAYOUT_VERSION=0.24.2

ENV DB=/db \
    EXTRA_CFLAGS=-march=generic \
    LOCALBUILDDIR=/tmp/build \
    LOCALDESTDIR=/tmp/local \
    PKG_CONFIG="pkg-config --static" \
    PKG_CONFIG_PATH="/usr/lib64/pkgconfig/:/tmp/local/lib/pkgconfig" \
    CPPFLAGS="-I/tmp/local/include -O3 -fno-strict-overflow -fstack-protector-all -fPIC" \
    CFLAGS="-I/tmp/local/include -O3 -fno-strict-overflow -fstack-protector-all -fPIC" \
    CXXFLAGS="-I/tmp/local/include -O2 -fPIC" \
    LDFLAGS="-L/tmp/local/lib -pipe -Wl,-z,relro,-z,now -static" \
    CC=clang

RUN dnf clean all -y && \
    dnf makecache --refresh && \
    dnf install -y epel-release && \
    dnf config-manager --set-enabled crb

RUN dnf install -y which sqlite libstdc++-static libtool autoconf clang \
    cmake ninja-build cargo ragel meson git pkgconfig bzip2 \
    python3-devel gperf perl glibc-static binutils-devel \
    nasm rsync wget zlib-devel

WORKDIR /tmp

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "zlib-1.3.1.tar.gz" "https://zlib.net/zlib-1.3.1.tar.gz" && \
    tar xf "zlib-1.3.1.tar.gz" && \
    cd "zlib-1.3.1" && \
    ./configure --prefix="$LOCALDESTDIR" --static && \
    make -j $(nproc) && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "openssl-1.1.1u.tar.gz" "https://www.openssl.org/source/openssl-1.1.1u.tar.gz" && \
    tar xf "openssl-1.1.1u.tar.gz" && \
    cd "openssl-1.1.1u" && \
    ./Configure --prefix=$LOCALDESTDIR --openssldir=$LOCALDESTDIR linux-x86_64 --libdir="$LOCALDESTDIR/lib" no-shared enable-camellia enable-idea enable-mdc2 enable-rfc3779 -static-libstdc++ -static-libgcc && \
    make depend all && \
    make install_sw

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "bzip2-1.0.8.tar.gz" "https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz" && \
    tar xf "bzip2-1.0.8.tar.gz" && \
    cd "bzip2-1.0.8" && \
    make install PREFIX="$LOCALDESTDIR"

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "libpng-1.6.40.tar.gz" "http://prdownloads.sourceforge.net/libpng/libpng-1.6.40.tar.gz" && \
    tar xf "libpng-1.6.40.tar.gz" && \
    cd "libpng-1.6.40" && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared && \
    make -j $(nproc) && \
    make install

RUN git clone --depth 1 "https://github.com/fribidi/fribidi.git" && cd fribidi && \
    mkdir build && cd build && \
    meson setup -Ddocs=false -Dbin=false --default-library=static .. --prefix "$LOCALDESTDIR" --libdir="$LOCALDESTDIR/lib" && \
    ninja && ninja install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "expat-2.5.0.tar.bz2" "https://github.com/libexpat/libexpat/releases/download/R_2_5_0/expat-2.5.0.tar.bz2" && \
    tar xf "expat-2.5.0.tar.bz2" && \
    cd "expat-2.5.0" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no --without-docbook && \
    make -j $(nproc) && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "freetype-2.13.1.tar.gz" "https://sourceforge.net/projects/freetype/files/freetype2/2.13.1/freetype-2.13.1.tar.gz" && \
    tar xf "freetype-2.13.1.tar.gz" && \
    cd "freetype-2.13.1" && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared --with-harfbuzz=no && \
    make -j $(nproc) && \
    make install

RUN curl --retry 20 --retry-max-time 5 -L -k -f -w "%{response_code}" -o "fontconfig-2.14.2.tar.gz" "https://www.freedesktop.org/software/fontconfig/release/fontconfig-2.14.2.tar.gz" && \
    tar xf "fontconfig-2.14.2.tar.gz" && \
    cd "fontconfig-2.14.2" && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no && \
    make -j $(nproc) && \
    make install && \
    cp fontconfig.pc "$LOCALDESTDIR/lib/pkgconfig/"

RUN git clone --depth 1 "https://github.com/harfbuzz/harfbuzz.git" && cd harfbuzz && \
    mkdir build && cd build && \
    meson setup -Denable_tools=false --default-library=static .. --prefix "$LOCALDESTDIR" --libdir="$LOCALDESTDIR/lib" && \
    ninja && \
    ninja install

RUN git clone --depth 1 "https://github.com/zeromq/libzmq.git" && cd libzmq && \
    ./autogen.sh && \
    ./configure --prefix="$LOCALDESTDIR" --enable-static --disable-shared && \
    make -j $(nproc) && \
    make install

RUN git clone --depth 1 "https://github.com/libass/libass.git" && cd libass && \
    ./autogen.sh && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no --disable-harfbuzz && \
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
    cmake .. -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DOPENSSL_USE_STATIC_LIBS=ON -DUSE_STATIC_LIBSTDCXX:BOOLEAN=ON -DENABLE_CXX11:BOOLEAN=ON -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" -DENABLE_APPS=0 -DENABLE_EXAMPLES=0 && \
    make -j $(nproc) && \
    make install

RUN git clone "https://github.com/webmproject/libvpx.git" && cd libvpx && \
    ./configure --prefix="$LOCALDESTDIR" --as=nasm --disable-shared --enable-static --disable-unit-tests --disable-docs --enable-postproc --enable-vp9-postproc --enable-runtime-cpu-detect && \
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

RUN git clone --depth 1 "https://gitlab.com/AOMediaCodec/SVT-AV1.git" && cd SVT-AV1/Build && \
    cmake .. -G"Unix Makefiles" -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DSVT_AV1_LTO=OFF -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j $(nproc) && \
    make install

RUN git clone --depth 1 "https://code.videolan.org/videolan/dav1d.git" && cd dav1d && \
    mkdir build && cd build && \
    meson setup -Denable_tools=false -Denable_tests=false --default-library=static .. --prefix "$LOCALDESTDIR" --libdir="$LOCALDESTDIR/lib" && \
    ninja && \
    ninja install

RUN git clone --depth 1 https://git.videolan.org/git/ffmpeg/nv-codec-headers && cd nv-codec-headers && \
    make install PREFIX="$LOCALDESTDIR"

RUN git clone --depth 1 https://git.ffmpeg.org/ffmpeg.git && cd ffmpeg && \
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
    --enable-gpl \
    --enable-version3 \
    --enable-nonfree \
    --enable-avfilter \
    --enable-zlib \
    --enable-static \
    --enable-libass \
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
    --enable-nonfree \
    --enable-openssl \
    --enable-libsvtav1 \
    --enable-libdav1d \
    --enable-nvenc || echo -e "\n\nTRACE:\n" && tail -50 ffbuild/config.log && echo -e "\n\n" && \
    make -j $(nproc) && \
    make install && \
    strip /usr/local/bin/ffmpeg /usr/local/bin/ffprobe

WORKDIR /

COPY README.md ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.* /tmp/

COPY <<-EOT /run.sh
#!/bin/sh

if [ ! -f /db/ffplayout.db ]; then
    ffplayout -i -u admin -p admin -m contact@example.com --storage "/tv-media" --playlists "/playlists" --public "/public" --logs "/logging" --smtp-server "mail.example.org" --smtp-user "admin@example.org" --smtp-password "" --smtp-port 465 --smtp-starttls false
fi

/usr/bin/ffplayout -l "0.0.0.0:8787"
EOT

RUN chmod +x /run.sh

RUN [[ -f "/tmp/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" ]] || \
    wget "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" -P /tmp/

RUN cd /tmp && \
    tar xf "ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" && \
    cp ffplayout /usr/bin/ && \
    mkdir -p /usr/share/ffplayout/ && \
    cp assets/dummy.vtt assets/logo.png assets/DejaVuSans.ttf assets/FONT_LICENSE.txt /usr/share/ffplayout/ && \
    rm -rf /tmp/* && \
    mkdir ${DB}

EXPOSE 8787

CMD ["/run.sh"]
