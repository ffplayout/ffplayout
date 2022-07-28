#!/usr/bin/bash

yes | rm -f assets/ffplayout.1.gz
yes | rm -f assets/ffpapi.1.gz

engine_docs=(
    "README.md"
    "ffplayout-engine/README.md"
    "docs/install.md"
    "docs/output.md"
    "docs/live_ingest.md"
    "docs/preview_stream.md"
)

api_docs=(
    "ffplayout-api/README.md"
    "docs/api.md"
)

pandoc "${engine_docs[@]}" -s --wrap=preserve -t man -o assets/ffplayout.1
pandoc "${api_docs[@]}" -s --wrap=preserve -t man -o assets/ffpapi.1

gzip assets/ffplayout.1
gzip assets/ffpapi.1
