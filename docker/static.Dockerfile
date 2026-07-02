FROM localhost/ffplayout-ffmpeg-static:latest

ARG CARGO_FEATURES=embed_frontend

ENV DEBIAN_FRONTEND=noninteractive \
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

CMD ["sh", "-c", "set -eux && echo 'Install frontend dependencies' && npm ci && echo 'Build frontend' && npm run build-only && echo 'Build ffplayout binary' && cargo build --release --package ffplayout --no-default-features --features \"$CARGO_FEATURES\" && version=\"$(sed -n 's/^version = \"\\(.*\\)\"/\\1/p' Cargo.toml | head -1)\" && echo 'Copy ffplayout binary' && mkdir -p /artifacts && cp target/release/ffplayout /artifacts/ffplayout && echo 'Build deb package' && cargo deb --no-build -p ffplayout --manifest-path backend/app/Cargo.toml -o \"/artifacts/ffplayout_${version}-1_amd64.deb\" && echo 'Artifacts written to /artifacts'"]
