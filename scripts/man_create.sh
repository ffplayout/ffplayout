#!/usr/bin/bash

yes | rm -f assets/ffplayout.1.gz

engine_docs=(
    "docs/README.md"
    "docs/api.md"
    "docs/closed_captions.md"
    "docs/folder_mode.md"
    "docs/ingest_error.md"
    "docs/install.md"
    "docs/live_ingest.md"
    "docs/output.md"
    "docs/playlist_gen.md"
    "docs/remote_source.md"
    "docs/developer.md"
)

pandoc "${engine_docs[@]}" -s --wrap=preserve -t man -o assets/ffplayout.1

gzip assets/ffplayout.1
