#!/usr/bin/bash

yes | rm -f assets/ffplayout.1.gz

engine_docs=(
    "README.md"
    "docs/api.md"
    "docs/install.md"
    "docs/output.md"
    "docs/live_ingest.md"
    "docs/preview_stream.md"
)

pandoc "${engine_docs[@]}" -s --wrap=preserve -t man -o assets/ffplayout.1

gzip assets/ffplayout.1
