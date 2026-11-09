#!/usr/bin/env bash
set -euo pipefail

package=${1:?Usage: check-deb.sh PACKAGE dynamic|static VERSION ARCHITECTURE}
linkage=${2:?Missing FFmpeg linkage mode}
version=${3:?Missing expected version}
architecture=${4:?Missing expected architecture}

case "$linkage" in
    dynamic|static) ;;
    *) echo "Unknown FFmpeg linkage mode: $linkage" >&2; exit 1 ;;
esac

[[ $(dpkg-deb -f "$package" Package) == ffplayout ]]
[[ $(dpkg-deb -f "$package" Version) == "$version" ]]
[[ $(dpkg-deb -f "$package" Architecture) == "$architecture" ]]

directory=$(mktemp -d)
trap 'rm -rf -- "$directory"' EXIT
dpkg-deb -x "$package" "$directory"
needed=$(readelf -d "$directory/usr/bin/ffplayout")
dependencies=$(dpkg-deb -f "$package" Depends)
ffmpeg_libraries='lib(avcodec|avformat|avutil|avfilter|avdevice|swscale|swresample)'

printf 'Package: %s\nDepends: %s\n' "$package" "$dependencies"

if [[ $linkage == static ]]; then
    if grep -Eq "$ffmpeg_libraries" <<< "$needed $dependencies"; then
        echo 'Static FFmpeg package unexpectedly depends on FFmpeg shared libraries.' >&2
        exit 1
    fi
else
    # These five libraries are required by every dynamically linked engine build.
    for library in avcodec avformat avutil swscale swresample; do
        grep -Eq "Shared library: \[lib${library}\.so\." <<< "$needed"
        grep -Eq "(^|[ ,])lib${library}[0-9]+([ :,(]|$)" <<< "$dependencies"
    done
fi
