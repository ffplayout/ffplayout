FROM rust:slim-trixie AS build
WORKDIR /src

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_FEATURES=""

RUN apt-get update && \
    apt-get --assume-yes install --no-install-recommends \
        ca-certificates \
        clang \
        curl \
        gnupg \
        libavcodec-dev \
        libavdevice-dev \
        libavfilter-dev \
        libavformat-dev \
        libavutil-dev \
        libasound2-dev \
        libclang-dev \
        libsqlite3-dev \
        libswresample-dev \
        libswscale-dev \
        pkg-config && \
    curl -fsSL https://deb.nodesource.com/setup_24.x | bash - && \
    apt-get --assume-yes install --no-install-recommends nodejs && \
    npm install -g npm && \
    cargo install cargo-deb --locked && \
    rm -rf /var/lib/apt/lists/*

CMD ["sh", "-c", "set -eux && echo 'Install frontend dependencies' && npm ci && echo 'Build frontend' && npm run build-only && echo 'Build ffplayout binary' && if [ -n \"$CARGO_FEATURES\" ]; then cargo build --release --package ffplayout --features \"$CARGO_FEATURES\"; else cargo build --release --package ffplayout; fi && version=\"$(sed -n 's/^version = \"\\(.*\\)\"/\\1/p' Cargo.toml | head -1)\" && echo 'Build deb package' && cargo deb --no-build -p ffplayout --manifest-path crates/app/Cargo.toml -o \"/src/ffplayout_${version}-1_amd64.deb\""]
