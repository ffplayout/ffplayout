FROM rust:slim-trixie AS build
WORKDIR /src

ENV DEBIAN_FRONTEND=noninteractive

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
