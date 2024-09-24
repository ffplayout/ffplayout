FROM alpine:latest

ARG FFPLAYOUT_VERSION=0.24.0-rc1
ARG SHARED_STORAGE=false

ENV DB=/db
ENV SHARED_STORAGE=${SHARED_STORAGE}

COPY --from=ffmpeg-build /usr/local/bin/ffmpeg /usr/local/bin/ffmpeg
COPY --from=ffmpeg-build /usr/local/bin/ffprobe /usr/local/bin/ffprobe
COPY README.md ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.* /tmp/

COPY <<-EOT /run.sh
#!/bin/sh

if [ ! -f /db/ffplayout.db ]; then
    ffplayout -u admin -p admin -m contact@example.com --storage-root "/tv-media" --playlist-root "/playlists" --public "/public" --log-path "/logging" --shared-storage
fi

/usr/bin/ffplayout -l "0.0.0.0:8787"
EOT

RUN apk update && \
    apk upgrade && \
    apk add --no-cache sqlite font-dejavu && \
    chmod +x /run.sh

RUN [[ -f "/tmp/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" ]] || \
    wget -q "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" -P /tmp/ && \
    cd /tmp && \
    tar xf "ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" && \
    cp ffplayout /usr/bin/ && \
    rm -rf /tmp/* && \
    mkdir ${DB}

EXPOSE 8787

CMD ["/run.sh"]
