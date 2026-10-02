#!/usr/bin/bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

engine_docs=(docs/README.md)
for doc in docs/*.md; do
    [[ $doc == docs/README.md ]] || engine_docs+=("$doc")
done

tmp_dir=$(mktemp -d assets/.ffplayout-man.XXXXXX)
trap 'rm -f -- "$tmp_dir/ffplayout.1" "$tmp_dir/ffplayout.1.gz"; rmdir -- "$tmp_dir"' EXIT

pandoc "${engine_docs[@]}" -s --wrap=preserve -t man \
    -M title=ffplayout -M section=1 -o "$tmp_dir/ffplayout.1"
gzip -n -c "$tmp_dir/ffplayout.1" > "$tmp_dir/ffplayout.1.gz"
mv -f -- "$tmp_dir/ffplayout.1.gz" assets/ffplayout.1.gz
