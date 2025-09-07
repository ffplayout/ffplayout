FROM rust:slim-bookworm AS build
WORKDIR /src

RUN apt-get update && apt-get --assume-yes install curl && \
    curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && \
    apt-get --assume-yes install nodejs && npm install -g npm
RUN cargo install cargo-deb
