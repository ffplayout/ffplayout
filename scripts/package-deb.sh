#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
source scripts/lib/deb-package.sh

output_directory=${1:?Usage: package-deb.sh OUTPUT_DIRECTORY dynamic|static [VERSION]}
linkage=${2:?Missing FFmpeg linkage mode}
version=${3:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
distribution=$(deb_distribution_suffix /etc/os-release)
package_version=$(deb_package_version "$version" "$distribution")
architecture=$(dpkg --print-architecture)
package="$output_directory/ffplayout_${package_version}_${architecture}.deb"
args=(--no-build -p ffplayout --manifest-path crates/app/Cargo.toml --deb-version "$package_version" -o "$package")

if [[ ${CARGO_PROFILE_RELEASE_STRIP:-} == false ]]; then
    args+=(--no-strip)
fi

if [[ $architecture == arm64 ]]; then
    args+=(--variant arm64)
fi

mkdir -p "$output_directory"
cargo deb "${args[@]}"
bash scripts/check-deb.sh "$package" "$linkage" "$package_version" "$architecture"
