FROM almalinux:9 AS base

ENV container docker

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

ENV SRC=/usr/local \
    BUILD=/tmp/build

ARG LD_LIBRARY_PATH=/opt/ffmpeg/lib
ARG PKG_CONFIG_PATH="/opt/ffmpeg/share/pkgconfig:/opt/ffmpeg/lib/pkgconfig:/opt/ffmpeg/lib64/pkgconfig:/lib64/pkgconfig"
ARG LOCALDESTDIR=/opt/ffmpeg
ARG LD_LIBRARY_PATH="/opt/ffmpeg/lib:/opt/ffmpeg/lib64"

RUN \
    buildDeps="bzip2 gperf which libticonv autoconf automake cmake diffutils file gcc \
    ninja-build wget nasm gcc-c++ git libtool make perl yasm meson x264-devel zlib-devel \
    expat-devel fontconfig-devel libxml2-devel lame-devel libpng-devel numactl-devel \
    fribidi-devel zeromq-devel freetype-devel opus-devel libass-devel openssl-devel" && \
    echo "${SRC}/lib" > /etc/ld.so.conf.d/libc.conf && \
    dnf install -y epel-release && \
    dnf install -y 'dnf-command(config-manager)' && \
    dnf config-manager --set-enabled crb && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/free/el/rpmfusion-free-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y ${buildDeps} && \
    mkdir -p ${BUILD}

RUN \
    cd ${BUILD} && \
    git clone --depth 1 "https://github.com/Haivision/srt.git" && \
    cd srt && \
    mkdir build && \
    cd build && \
    cmake .. -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DUSE_STATIC_LIBSTDCXX:BOOLEAN=ON \
        -DENABLE_CXX11:BOOLEAN=OFF -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install

RUN \
    cd ${BUILD} && \
    git clone --depth 1 "https://code.videolan.org/rist/librist.git" && \
    cd librist && \
    mkdir build && \
    cd build && \
    meson setup --default-library=static --prefix "$LOCALDESTDIR" --libdir="$LOCALDESTDIR/lib" .. && \
    ninja && \
    ninja install

RUN \
    cd ${BUILD} && \
    git clone --depth 1 "https://github.com/mstorsjo/fdk-aac" && \
    cd fdk-aac && \
    ./autogen.sh && \
    ./configure --prefix="$LOCALDESTDIR" --enable-shared=no && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install

RUN \
    cd ${BUILD} && \
    git clone --depth 1 "https://gitlab.com/AOMediaCodec/SVT-AV1.git" && \
    cd SVT-AV1/Build && \
    rm -rf * && \
    cmake .. -G"Unix Makefiles" -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DCMAKE_BUILD_TYPE=Release \
        -DBUILD_SHARED_LIBS=OFF -DCMAKE_INSTALL_BINDIR="bin" -DCMAKE_INSTALL_LIBDIR="lib" -DCMAKE_INSTALL_INCLUDEDIR="include" && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install

RUN \
    cd ${BUILD} && \
    git clone --depth 1 "https://code.videolan.org/videolan/dav1d.git" && \
    cd dav1d && \
    mkdir build && \
    cd build && \
    meson setup -Denable_tools=false -Denable_tests=false --default-library=static .. --prefix "$LOCALDESTDIR" --libdir="$LOCALDESTDIR/lib" && \
    ninja && \
    ninja install

RUN \
    cd ${BUILD} && \
    git clone "https://github.com/webmproject/libvpx.git" && \
    cd libvpx && \
    ./configure --prefix="$LOCALDESTDIR" --disable-shared --enable-static --disable-unit-tests --disable-docs --enable-postproc --enable-vp9-postproc --enable-runtime-cpu-detect && \
     make -j $(nproc | awk '{print $1 / 2}') && \
    make install

RUN \
    cd ${BUILD} && \
    git clone "https://bitbucket.org/multicoreware/x265_git.git" x265 && \
    cd x265/build && \
    rm -rf * && \
    cmake ../source -DCMAKE_INSTALL_PREFIX="$LOCALDESTDIR" -DENABLE_SHARED:BOOLEAN=OFF -DCMAKE_CXX_FLAGS_RELEASE:STRING="-O3 -DNDEBUG" && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install

RUN \
    cd ${BUILD} && \
    wget "https://ffmpeg.org/releases/ffmpeg-snapshot.tar.bz2" && \
    tar xfvj ffmpeg-snapshot.tar.bz2 && \
    rm -rf ffmpeg-snapshot.tar.bz2 && \
    cd ffmpeg && \
    ./configure --prefix="$LOCALDESTDIR" --enable-pthreads --extra-libs=-lpthread \
    --disable-debug --disable-shared --disable-doc --enable-gpl --enable-version3 --pkg-config-flags=--static \
    --enable-nonfree --enable-runtime-cpudetect --enable-fontconfig \
    --enable-openssl --enable-libass --enable-libfdk-aac --enable-libfreetype \
    --enable-libfribidi --enable-libmp3lame --enable-libopus --enable-libvpx --enable-librist \
    --enable-libsrt --enable-libx264 --enable-libx265 --enable-libzmq --enable-libsvtav1 --enable-libdav1d && \
    make -j $(nproc | awk '{print $1 / 2}') && \
    make install

RUN \
    cd / && \
    cp /opt/ffmpeg/bin/ff* /usr/local/bin/ && \
    rm -rf $BUILD $LOCALDESTDIR && \
    dnf -y remove autoconf automake cmake diffutils file gcc ninja-build nasm gcc-c++ git libtool make perl yasm meson \
    x264-devel zlib-devel expat-devel fontconfig-devel libxml2-devel lame-devel libpng-devel numactl-devel \
    fribidi-devel zeromq-devel freetype-devel opus-devel libass-devel openssl-devel && \
    dnf autoremove -y && \
    dnf clean all

FROM base

ARG FFPLAYOUT_VERSION=0.20.5

ENV LD_LIBRARY_PATH=/usr/local/lib64:/usr/local/lib

COPY --from=build /usr/local/ /usr/local/

ADD ./overide.conf /etc/systemd/system/ffplayout.service.d/overide.conf
ADD ./overide.conf /etc/systemd/system/ffpapi.service.d/overide.conf

RUN \
    dnf update -y \
    dnf install -y epel-release && \
    dnf install -y 'dnf-command(config-manager)' && \
    dnf config-manager --set-enabled crb && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/free/el/rpmfusion-free-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y wget dejavu-sans-fonts sudo x264-libs fontconfig lame libpng numactl fribidi zeromq freetype opus libass && \
    wget -q -O /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm" && \
    dnf install -y /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm && \
    dnf clean all && \
    rm /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm && \
    mkdir -p /home/ffpu && chown -R ffpu: /home/ffpu && \
    systemctl enable ffplayout && \
    systemctl enable ffpapi && \
    ffpapi -u admin -p admin -m contact@example.com

EXPOSE 8787

VOLUME [ "/sys/fs/cgroup" ]

CMD ["/usr/sbin/init"]
