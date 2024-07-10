#!/bin/sh

if [ ! -f /db/ffplayout.db ]; then
    ffplayout -u admin -p admin -m contact@example.com --storage-path "/tv-media" --playlist-path "/playlists" --hls-path "/hls" --log-path "/logging" --shared-storage
fi

/usr/bin/ffplayout -l "0.0.0.0:8787"
