FROM alpine:latest

ARG FFPLAYOUT_VERSION=0.24.0-alpha1
ARG SHARED_STORAGE=false

ENV DB=/db
ENV SHARED_STORAGE=${SHARED_STORAGE}

COPY README.md ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.* /tmp/

RUN apk update && \
    apk upgrade && \
    apk add --no-cache ffmpeg sqlite font-dejavu

RUN [[ -f "/tmp/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" ]] || \
    wget -q "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" -P /tmp/ && \
    cd /tmp && \
    tar xf "ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" && \
    cp ffplayout /usr/bin/ && \
    rm -rf /tmp/* && \
    mkdir ${DB}

RUN ffplayout -u admin -p admin -m contact@example.com --storage-path "/tv-media" --playlist-path "/playlists" --hls-path "/hls" --log-path "/logging"

EXPOSE 8787

CMD ["/usr/bin/ffplayout", "-l", "0.0.0.0:8787"]
