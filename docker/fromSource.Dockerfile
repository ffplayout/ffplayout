FROM centos:7 AS base

ENV container docker
RUN yum -y install libgomp && \
    yum clean all;

RUN (cd /lib/systemd/system/sysinit.target.wants/; for i in *; do [ $i == \
    systemd-tmpfiles-setup.service ] || rm -f $i; done); \
    rm -f /lib/systemd/system/multi-user.target.wants/*; \
    rm -f /etc/systemd/system/*.wants/*; \
    rm -f /lib/systemd/system/local-fs.target.wants/*; \
    rm -f /lib/systemd/system/sockets.target.wants/*udev*; \
    rm -f /lib/systemd/system/sockets.target.wants/*initctl*; \
    rm -f /lib/systemd/system/basic.target.wants/*; \
    rm -f /lib/systemd/system/anaconda.target.wants/*

FROM base AS build

WORKDIR /tmp/workdir

ENV FFMPEG_VERSION=5.1.2 \
    AOM_VERSION=v1.0.0 \
    FDKAAC_VERSION=0.1.5 \
    FONTCONFIG_VERSION=2.12.4 \
    FREETYPE_VERSION=2.10.4 \
    FRIBIDI_VERSION=0.19.7 \
    KVAZAAR_VERSION=2.0.0 \
    LAME_VERSION=3.100 \
    LIBASS_VERSION=0.13.7 \
    LIBPTHREAD_STUBS_VERSION=0.4 \
    OGG_VERSION=1.3.2 \
    OPUS_VERSION=1.2 \
    OPENJPEG_VERSION=2.1.2 \
    VORBIS_VERSION=1.3.5 \
    VPX_VERSION=1.8.0 \
    WEBP_VERSION=1.0.2 \
    X264_VERSION=20170226-2245-stable \
    X265_VERSION=3.4 \
    LIBZMQ_VERSION=4.3.2 \
    LIBSRT_VERSION=1.4.1 \
    LIBPNG_VERSION=1.6.9 \
    SRC=/usr/local

ARG FREETYPE_SHA256SUM="5eab795ebb23ac77001cfb68b7d4d50b5d6c7469247b0b01b2c953269f658dac freetype-2.10.4.tar.gz"
ARG FRIBIDI_SHA256SUM="3fc96fa9473bd31dcb5500bdf1aa78b337ba13eb8c301e7c28923fea982453a8 0.19.7.tar.gz"
ARG LIBASS_SHA256SUM="8fadf294bf701300d4605e6f1d92929304187fca4b8d8a47889315526adbafd7 0.13.7.tar.gz"
ARG OGG_SHA256SUM="e19ee34711d7af328cb26287f4137e70630e7261b17cbe3cd41011d73a654692 libogg-1.3.2.tar.gz"
ARG OPUS_SHA256SUM="77db45a87b51578fbc49555ef1b10926179861d854eb2613207dc79d9ec0a9a9 opus-1.2.tar.gz"
ARG VORBIS_SHA256SUM="6efbcecdd3e5dfbf090341b485da9d176eb250d893e3eb378c428a2db38301ce libvorbis-1.3.5.tar.gz"
ARG LIBZMQ_SHA256SUM="02ecc88466ae38cf2c8d79f09cfd2675ba299a439680b64ade733e26a349edeb v4.3.2.tar.gz"

ARG LD_LIBRARY_PATH=/opt/ffmpeg/lib
ARG MAKEFLAGS="-j2"
ARG PKG_CONFIG_PATH="/opt/ffmpeg/share/pkgconfig:/opt/ffmpeg/lib/pkgconfig:/opt/ffmpeg/lib64/pkgconfig"
ARG PREFIX=/opt/ffmpeg
ARG LD_LIBRARY_PATH="/opt/ffmpeg/lib:/opt/ffmpeg/lib64"

RUN buildDeps="autoconf \
    automake \
    bzip2 \
    cmake3 \
    diffutils \
    expat-devel \
    file \
    gcc \
    gcc-c++ \
    git \
    gperf \
    libtool \
    make \
    perl \
    python3 \
    openssl-devel \
    tar \
    yasm \
    which \
    zlib-devel" && \
    echo "${SRC}/lib" > /etc/ld.so.conf.d/libc.conf && \
    yum --enablerepo=extras install -y epel-release && \
    yum --enablerepo=epel install -y ${buildDeps} && \
    alternatives --install /usr/bin/cmake cmake /usr/bin/cmake3 0 && \
    # Install the tools required to build nasm 2.14.02 \
    nasmDeps="asciidoc \
        perl-Font-TTF \
        perl-Sort-Versions \
        xmlto" && \
    yum --enablerepo=epel install -y ${nasmDeps} && \
    # Compile and install nasm 2.14.02 \
    DIR=/tmp/nasm && \
    mkdir -p ${DIR} && \
    curl -LSs https://www.nasm.us/pub/nasm/releasebuilds/2.14.02/nasm-2.14.02.tar.gz | \
    tar xzC ${DIR} --strip-components=1 && \
    pushd ${DIR} && \
    ./configure --host=x86_64-redhat-linux-gnu \
        --build=x86_64-redhat-linux-gnu \
        --prefix=/usr/local \
        --exec-prefix=/usr/local \
        --bindir=/usr/local/bin \
        --sbindir=/usr/local/sbin \
        --sysconfdir=/usr/local/etc \
        --datadir=/usr/local/share \
        --includedir=/usr/local/include \
        --libdir=/usr/local/lib \
        --libexecdir=/usr/local/libexec \
        --enable-sections && \
    make all && \
    make install && \
    make install_rdf && \
    popd && rm -rf ${DIR} && \
    alternatives --install /usr/bin/nasm nasm /usr/local/bin/nasm 0 && \
    # Now that we have a modern nasm build and available, we can undo the last \
    # yum transaction as none of those packages are required for the rest of the build \
    yum history undo $(yum history info | grep 'Transaction ID' | awk -F: '{print$2}' | tr -d ' ') -y && \
    yum autoremove -y

## x264 http://www.videolan.org/developers/x264.html
RUN \
    DIR=/tmp/x264 && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sL https://download.videolan.org/pub/videolan/x264/snapshots/x264-snapshot-${X264_VERSION}.tar.bz2 | \
    tar -jx --strip-components=1 && \
    ./configure --prefix="${PREFIX}" --enable-shared --enable-pic --disable-cli && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

### x265 http://x265.org/
RUN \
    DIR=/tmp/x265 && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sL https://github.com/videolan/x265/archive/refs/tags/${X265_VERSION}.tar.gz | \
    tar -zx && \
    cd x265-${X265_VERSION}/build/linux && \
    sed -i "/-DEXTRA_LIB/ s/$/ -DCMAKE_INSTALL_PREFIX=\${PREFIX}/" multilib.sh && \
    sed -i "/^cmake/ s/$/ -DENABLE_CLI=OFF/" multilib.sh && \
    ./multilib.sh && \
    make -C 8bit install && \
    rm -rf ${DIR}

### libogg https://www.xiph.org/ogg/
RUN \
    DIR=/tmp/ogg && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO http://downloads.xiph.org/releases/ogg/libogg-${OGG_VERSION}.tar.gz && \
    echo ${OGG_SHA256SUM} | sha256sum --check && \
    tar -zx --strip-components=1 -f libogg-${OGG_VERSION}.tar.gz && \
    ./configure --prefix="${PREFIX}" --enable-shared  && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

### libopus https://www.opus-codec.org/
RUN \
    DIR=/tmp/opus && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://archive.mozilla.org/pub/opus/opus-${OPUS_VERSION}.tar.gz && \
    echo ${OPUS_SHA256SUM} | sha256sum --check && \
    tar -zx --strip-components=1 -f opus-${OPUS_VERSION}.tar.gz && \
    autoreconf -fiv && \
    ./configure --prefix="${PREFIX}" --enable-shared && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

### libvorbis https://xiph.org/vorbis/
RUN \
    DIR=/tmp/vorbis && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO http://downloads.xiph.org/releases/vorbis/libvorbis-${VORBIS_VERSION}.tar.gz && \
    echo ${VORBIS_SHA256SUM} | sha256sum --check && \
    tar -zx --strip-components=1 -f libvorbis-${VORBIS_VERSION}.tar.gz && \
    ./configure --prefix="${PREFIX}" --with-ogg="${PREFIX}" --enable-shared && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

### libvpx https://www.webmproject.org/code/
RUN \
    DIR=/tmp/vpx && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sL https://codeload.github.com/webmproject/libvpx/tar.gz/v${VPX_VERSION} | \
    tar -zx --strip-components=1 && \
    ./configure --prefix="${PREFIX}" --enable-vp8 --enable-vp9 --enable-vp9-highbitdepth --enable-pic --enable-shared \
    --disable-debug --disable-examples --disable-docs --disable-install-bins  && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

### libwebp https://developers.google.com/speed/webp/
RUN \
    DIR=/tmp/vebp && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sL https://storage.googleapis.com/downloads.webmproject.org/releases/webp/libwebp-${WEBP_VERSION}.tar.gz | \
    tar -zx --strip-components=1 && \
    ./configure --prefix="${PREFIX}" --enable-shared  && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

### libmp3lame http://lame.sourceforge.net/
RUN \
    DIR=/tmp/lame && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sL https://sourceforge.net/projects/lame/files/lame/${LAME_VERSION}/lame-${LAME_VERSION}.tar.gz/download | \
    tar -zx --strip-components=1 && \
    ./configure --prefix="${PREFIX}" --bindir="${PREFIX}/bin" --enable-shared --enable-nasm --disable-frontend && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

### fdk-aac https://github.com/mstorsjo/fdk-aac
RUN \
    DIR=/tmp/fdk-aac && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sL https://github.com/mstorsjo/fdk-aac/archive/v${FDKAAC_VERSION}.tar.gz | \
    tar -zx --strip-components=1 && \
    autoreconf -fiv && \
    ./configure --prefix="${PREFIX}" --enable-shared --datadir="${DIR}" && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

## openjpeg https://github.com/uclouvain/openjpeg
RUN \
    DIR=/tmp/openjpeg && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sL https://github.com/uclouvain/openjpeg/archive/v${OPENJPEG_VERSION}.tar.gz | \
    tar -zx --strip-components=1 && \
    cmake -DBUILD_THIRDPARTY:BOOL=ON -DCMAKE_INSTALL_PREFIX="${PREFIX}" . && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

## freetype https://www.freetype.org/
RUN  \
    DIR=/tmp/freetype && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://download.savannah.gnu.org/releases/freetype/freetype-${FREETYPE_VERSION}.tar.gz && \
    echo ${FREETYPE_SHA256SUM} | sha256sum --check && \
    tar -zx --strip-components=1 -f freetype-${FREETYPE_VERSION}.tar.gz && \
    ./configure --prefix="${PREFIX}" --disable-static --enable-shared && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

## fridibi https://www.fribidi.org/
RUN  \
    DIR=/tmp/fribidi && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://github.com/fribidi/fribidi/archive/${FRIBIDI_VERSION}.tar.gz && \
    echo ${FRIBIDI_SHA256SUM} | sha256sum --check && \
    tar -zx --strip-components=1 -f ${FRIBIDI_VERSION}.tar.gz && \
    sed -i 's/^SUBDIRS =.*/SUBDIRS=gen.tab charset lib bin/' Makefile.am && \
    ./bootstrap --no-config --auto && \
    ./configure --prefix="${PREFIX}" --disable-static --enable-shared && \
    make -j1 && \
    make install && \
    rm -rf ${DIR}

## fontconfig https://www.freedesktop.org/wiki/Software/fontconfig/
RUN  \
    DIR=/tmp/fontconfig && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://www.freedesktop.org/software/fontconfig/release/fontconfig-${FONTCONFIG_VERSION}.tar.bz2 && \
    tar -jx --strip-components=1 -f fontconfig-${FONTCONFIG_VERSION}.tar.bz2 && \
    ./configure --prefix="${PREFIX}" --disable-static --enable-shared && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

## libass https://github.com/libass/libass
RUN  \
    DIR=/tmp/libass && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://github.com/libass/libass/archive/${LIBASS_VERSION}.tar.gz && \
    echo ${LIBASS_SHA256SUM} | sha256sum --check && \
    tar -zx --strip-components=1 -f ${LIBASS_VERSION}.tar.gz && \
    ./autogen.sh && \
    ./configure --prefix="${PREFIX}" --disable-static --enable-shared && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

## kvazaar https://github.com/ultravideo/kvazaar
RUN \
    DIR=/tmp/kvazaar && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://github.com/ultravideo/kvazaar/archive/v${KVAZAAR_VERSION}.tar.gz && \
    tar -zx --strip-components=1 -f v${KVAZAAR_VERSION}.tar.gz && \
    ./autogen.sh && \
    ./configure --prefix="${PREFIX}" --disable-static --enable-shared && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

RUN \
    DIR=/tmp/aom && \
    git clone --branch ${AOM_VERSION} --depth 1 https://aomedia.googlesource.com/aom ${DIR} && \
    cd ${DIR} && \
    rm -rf CMakeCache.txt CMakeFiles && \
    mkdir -p ./aom_build && \
    cd ./aom_build && \
    cmake -DCMAKE_INSTALL_PREFIX="${PREFIX}" -DBUILD_SHARED_LIBS=1 .. && \
    make && \
    make install && \
    rm -rf ${DIR}

RUN \
    DIR=/tmp/libpthread-stubs && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://xcb.freedesktop.org/dist/libpthread-stubs-${LIBPTHREAD_STUBS_VERSION}.tar.gz && \
    tar -zx --strip-components=1 -f libpthread-stubs-${LIBPTHREAD_STUBS_VERSION}.tar.gz && \
    ./configure --prefix="${PREFIX}" && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

## libzmq https://github.com/zeromq/libzmq/
RUN \
    DIR=/tmp/libzmq && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://github.com/zeromq/libzmq/archive/v${LIBZMQ_VERSION}.tar.gz && \
    echo ${LIBZMQ_SHA256SUM} | sha256sum --check && \
    tar -xz --strip-components=1 -f v${LIBZMQ_VERSION}.tar.gz && \
    ./autogen.sh && \
    ./configure --prefix="${PREFIX}" && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make check && \
    make install && \
    rm -rf ${DIR}

## libsrt https://github.com/Haivision/srt
RUN \
    DIR=/tmp/srt && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    curl -sLO https://github.com/Haivision/srt/archive/v${LIBSRT_VERSION}.tar.gz && \
    tar -xz --strip-components=1 -f v${LIBSRT_VERSION}.tar.gz && \
    cmake -DCMAKE_INSTALL_PREFIX="${PREFIX}" . && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    rm -rf ${DIR}

## libpng
RUN \
    DIR=/tmp/png && \
    mkdir -p ${DIR} && \
    cd ${DIR} && \
    git clone https://git.code.sf.net/p/libpng/code ${DIR} -b v${LIBPNG_VERSION} --depth 1 && \
    ./autogen.sh && \
    ./configure --prefix="${PREFIX}" && \
    make check && \
    make install && \
    rm -rf ${DIR}

## Download ffmpeg https://ffmpeg.org/
RUN  \
    DIR=/tmp/ffmpeg && mkdir -p ${DIR} && cd ${DIR} && \
    curl -sLO https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.bz2 && \
    tar -jx --strip-components=1 -f ffmpeg-${FFMPEG_VERSION}.tar.bz2 && \
    ./configure --disable-debug --disable-doc --disable-ffplay --enable-shared --enable-gpl --extra-libs=-ldl && \
    make && make install

## Build ffmpeg https://ffmpeg.org/
RUN  \
    DIR=/tmp/ffmpeg && cd ${DIR} && \
    ./configure \
    --disable-debug \
    --disable-doc \
    --disable-ffplay \
    --enable-gpl \
    --enable-libaom \
    --enable-libass \
    --enable-libfdk_aac \
    --enable-libfreetype \
    --enable-libmp3lame \
    --enable-libopenjpeg \
    --enable-libopus \
    --enable-libsrt \
    --enable-libvorbis \
    --enable-libvpx \
    --enable-libwebp \
    --enable-libx264 \
    --enable-libx265 \
    --enable-libzmq \
    --enable-nonfree \
    --enable-fontconfig \
    --enable-postproc \
    --enable-shared \
    --enable-version3 \
    --extra-cflags="-I${PREFIX}/include" \
    --extra-ldflags="-L${PREFIX}/lib" \
    --extra-libs=-ldl \
    --extra-libs=-lpthread \
    --prefix="${PREFIX}" && \
    make clean && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install && \
    make tools/zmqsend && cp tools/zmqsend ${PREFIX}/bin/ && \
    make distclean && \
    hash -r && \
    cd tools && \
    make qt-faststart && cp qt-faststart ${PREFIX}/bin/

RUN \
    ldd ${PREFIX}/bin/ffmpeg | grep opt/ffmpeg | cut -d ' ' -f 3 | xargs -i cp {} /usr/local/lib64/ && \
    for lib in /usr/local/lib64/*.so.*; do ln -s "${lib##*/}" "${lib%%.so.*}".so; done && \
    cp ${PREFIX}/bin/* /usr/local/bin/ && \
    cp -r ${PREFIX}/share/ffmpeg /usr/local/share/ && \
    LD_LIBRARY_PATH=/usr/local/lib64:/usr/local/lib ffmpeg -buildconf && \
    cp -r ${PREFIX}/include/libav* ${PREFIX}/include/libpostproc ${PREFIX}/include/libsw* /usr/local/include && \
    mkdir -p /usr/local/lib64/pkgconfig && \
    for pc in ${PREFIX}/lib/pkgconfig/libav*.pc ${PREFIX}/lib/pkgconfig/libpostproc.pc ${PREFIX}/lib/pkgconfig/libsw*.pc; do \
        sed "s:${PREFIX}:/usr/local:g" <"$pc" >/usr/local/lib64/pkgconfig/"${pc##*/}"; \
    done

FROM base

ARG FFPLAYOUT_VERSION=0.17.0

ENV LD_LIBRARY_PATH=/usr/local/lib64:/usr/local/lib

COPY --from=build /usr/local/ /usr/local/

ADD ./overide.conf /etc/systemd/system/ffplayout.service.d/overide.conf
ADD ./overide.conf /etc/systemd/system/ffpapi.service.d/overide.conf

RUN \
    yum update -y \
    && yum install -y wget dejavu-sans-fonts sudo \
    && wget -q -O /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm" \
    && yum install -y /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm \
    && yum clean all \
    && rm /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm \
    && mkdir -p /home/ffpu && chown -R ffpu: /home/ffpu \
    && systemctl enable ffplayout \
    && systemctl enable ffpapi \
    && ffpapi -u admin -p admin -m contact@example.com

EXPOSE 8787

VOLUME [ "/sys/fs/cgroup" ]

CMD ["/usr/sbin/init"]
