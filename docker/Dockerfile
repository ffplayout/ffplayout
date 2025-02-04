FROM alpine:latest

ARG FFPLAYOUT_VERSION=0.24.2

ENV DB=/db

COPY README.md ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.* /tmp/

COPY <<-EOT /run.sh
#!/bin/sh

if [ ! -f /db/ffplayout.db ]; then
    ffplayout -i -u admin -p admin -m contact@example.com --storage "/tv-media" --playlists "/playlists" --public "/public" --logs "/logging" --smtp-server "mail.example.org" --smtp-user "admin@example.org" --smtp-password "" --smtp-port 465 --smtp-starttls false
fi

/usr/bin/ffplayout -l "0.0.0.0:8787"
EOT

RUN apk update && \
    apk upgrade && \
    apk add --no-cache ffmpeg sqlite font-dejavu && \
    chmod +x /run.sh

RUN [[ -f "/tmp/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" ]] || \
    wget -q "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" -P /tmp/ && \
    cd /tmp && \
    tar xf "ffplayout-v${FFPLAYOUT_VERSION}_x86_64-unknown-linux-musl.tar.gz" && \
    cp ffplayout /usr/bin/ && \
    mkdir -p /usr/share/ffplayout/ && \
    cp assets/dummy.vtt assets/logo.png assets/DejaVuSans.ttf assets/FONT_LICENSE.txt /usr/share/ffplayout/ && \
    rm -rf /tmp/* && \
    mkdir ${DB}

EXPOSE 8787

CMD ["/run.sh"]
