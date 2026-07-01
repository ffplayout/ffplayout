FROM rust:1-trixie

ARG DEBIAN_FRONTEND=noninteractive

RUN set -eux; \
    printf '%s\n' \
        'Types: deb' \
        'URIs: http://deb.debian.org/debian' \
        'Suites: unstable' \
        'Components: main contrib non-free non-free-firmware' \
        'Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg' \
        > /etc/apt/sources.list.d/debian-unstable.sources; \
    printf '%s\n' \
        'Package: *' \
        'Pin: release a=unstable' \
        'Pin-Priority: 100' \
        > /etc/apt/preferences.d/limit-unstable; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        clang \
        curl \
        git \
        libclang-dev \
        libsqlite3-dev \
        pkg-config; \
    apt-get install -y --no-install-recommends -t unstable \
        nodejs \
        npm \
        libavcodec-dev \
        libavdevice-dev \
        libavfilter-dev \
        libavformat-dev \
        libavutil-dev \
        libswresample-dev \
        libswscale-dev; \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock package.json package-lock.json tsconfig.json vite.config.ts README.md LICENSE ./
COPY backend ./backend
COPY frontend ./frontend
COPY assets ./assets
COPY debian ./debian
COPY migrations ./migrations
COPY tests ./tests

CMD ["sh", "-c", "cargo build --release --package ffplayout --no-default-features --features embed_frontend && mkdir -p /artifacts && cp target/release/ffplayout /artifacts/ffplayout"]
