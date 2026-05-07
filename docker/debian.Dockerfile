FROM rust:slim-trixie AS build
WORKDIR /src

RUN apt-get update && apt-get --assume-yes install curl && \
    curl -fsSL https://deb.nodesource.com/setup_24.x | bash - && \
    apt-get --assume-yes install nodejs && npm install -g npm
RUN cargo install cargo-deb
