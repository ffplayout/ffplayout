FROM rust:slim-trixie AS build
WORKDIR /src

ARG CARGO_FEATURES=embed_frontend
ARG FFMPEG_AVDEVICE=0
ARG FFMPEG_AVFILTER=0
ARG FFMPEG_VAAPI=0

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_FEATURES="$CARGO_FEATURES"

RUN apt-get update && \
    apt-get --assume-yes install --no-install-recommends \
        ca-certificates \
        clang \
        dpkg-dev \
        binutils \
        curl \
        gnupg \
        libavcodec-dev \
        libavformat-dev \
        libavutil-dev \
        libclang-dev \
        libsqlite3-dev \
        libswresample-dev \
        libswscale-dev \
        pkg-config && \
    if [ "$FFMPEG_AVDEVICE" = 1 ]; then apt-get install -y --no-install-recommends libavdevice-dev; fi && \
    if [ "$FFMPEG_AVFILTER" = 1 ]; then apt-get install -y --no-install-recommends libavfilter-dev; fi && \
    if [ "$FFMPEG_VAAPI" = 1 ]; then apt-get install -y --no-install-recommends libva-dev libdrm-dev; fi && \
    case ",$CARGO_FEATURES," in *desktop*) apt-get install -y --no-install-recommends libasound2-dev ;; esac && \
    curl -fsSL https://deb.nodesource.com/setup_24.x | bash - && \
    apt-get --assume-yes install --no-install-recommends nodejs && \
    npm install -g npm && \
    cargo install cargo-deb --locked && \
    rm -rf /var/lib/apt/lists/*

CMD ["sh", "-c", "set -eux && echo 'Install frontend dependencies' && npm ci && echo 'Build frontend' && npm run build-only && echo 'Build ffplayout binary' && cargo build --release --package ffplayout --no-default-features --features \"$CARGO_FEATURES\" && version=\"$(sed -n 's/^version = \"\\(.*\\)\"/\\1/p' Cargo.toml | head -1)\" && echo 'Build deb package' && bash scripts/package-deb.sh /src dynamic \"$version\""]
