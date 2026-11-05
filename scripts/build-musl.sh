#!/bin/sh

# Runs inside docker/musl.Dockerfile, with the workspace mounted at /src.
set -eu

features=${CARGO_FEATURES:-embed_frontend}
if printf '%s' "$features" | grep -Eq '(^|[,[:space:]])([^,[:space:]]*/)?desktop(-[[:alnum:]_-]+)?([,[:space:]]|$)'; then
    echo 'The musl builder supports server outputs only; remove desktop features.' >&2
    exit 1
fi

if [ "${FFMPEG_VAAPI:-0}" != 0 ]; then
    echo 'The fully static musl builder does not support VAAPI or Intel VPL.' >&2
    exit 1
fi

# Use an explicit target and an isolated cache, even when .env overrides Cargo
# defaults. This also keeps target-only Rust flags away from host proc macros.
# A separate cache also invalidates old, statically linked host build scripts.
export CARGO_TARGET_DIR=/src/target/alpine-musl-host-dynamic
export RUSTC_WRAPPER=/usr/local/bin/rustc-musl-host
rust_target=x86_64-unknown-linux-musl
binary="$CARGO_TARGET_DIR/$rust_target/release/ffplayout"

# npm ci must not replace the host's node_modules with Alpine-native packages.
frontend_work=/tmp/ffplayout-musl-frontend
mkdir -p "$frontend_work"
rm -rf "$frontend_work/frontend"
tar -cf - --exclude=dist --exclude=node_modules \
    frontend package.json package-lock.json vite.config.ts tsconfig.json | \
    tar -xf - -C "$frontend_work"
echo 'Build frontend in isolated Alpine workspace'
(cd "$frontend_work" && npm ci --include=optional && npm run build)
rm -rf frontend/dist
mkdir -p frontend/dist
cp -R "$frontend_work/frontend/dist/." frontend/dist/

echo 'Build ffplayout for musl'
FFPLAYOUT_FRONTEND_PREBUILT=1 cargo build --locked --release --target "$rust_target" --package ffplayout \
    --no-default-features --features "$features"

echo 'Verify fully static ELF binary'
readelf -lW "$binary" > /tmp/ffplayout-musl-program-headers.txt
readelf -dW "$binary" > /tmp/ffplayout-musl-dynamic-section.txt
if grep -q INTERP /tmp/ffplayout-musl-program-headers.txt || \
    grep -q '(NEEDED)' /tmp/ffplayout-musl-dynamic-section.txt; then
    echo 'Refusing to publish a musl binary with dynamic runtime dependencies.' >&2
    exit 1
fi
"$binary" --version

mkdir -p /artifacts
cp "$binary" /artifacts/ffplayout
echo 'Static musl binary written to /artifacts/ffplayout'
