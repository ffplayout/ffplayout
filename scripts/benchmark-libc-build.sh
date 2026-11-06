#!/bin/sh
# Called by benchmark-libc.py inside the existing FFmpeg builder images.
set -eu

variant=$1
case "$variant" in
    glibc) triple=x86_64-unknown-linux-gnu ;;
    musl)
        triple=x86_64-unknown-linux-musl
        cp /src/scripts/rustc-musl-host.sh /tmp/benchmark-rustc-host
        chmod +x /tmp/benchmark-rustc-host
        export RUSTC_WRAPPER=/tmp/benchmark-rustc-host
        ;;
    *) echo "Unknown libc: $variant" >&2; exit 1 ;;
esac

# Use the same release profile and generic x86-64 CPU settings for both builds.
# The host wrapper affects build tools only; musl targets retain +crt-static.
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_TARGET
export CARGO_TARGET_DIR="/results/build/$variant/${LIBC_BENCH_CACHE_KEY:?missing builder image cache key}"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='-C target-cpu=x86-64'
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS='-C target-cpu=x86-64 -C target-feature=+crt-static'
export CARGO_PROFILE_RELEASE_DEBUG=false CARGO_PROFILE_RELEASE_STRIP=false
export CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_LTO=true
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_PANIC=abort
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
mkdir -p "/results/bin/$variant"
cd /src

{
    rustc -vV
    gcc --version
    cat /etc/os-release
    pkg-config --modversion libavutil libavcodec libavformat libswscale
    git -C /tmp/FFmpeg rev-parse HEAD 2>/dev/null || true
} > "/results/bin/$variant/environment.txt"

for suite in graphics audio; do
    case "$suite" in
        graphics) package=ff-graphics; features=ffmpeg ;;
        audio) package=ff-engine; features='' ;;
    esac

    cargo bench --locked --no-run --profile release --target "$triple" \
        --package "$package" --no-default-features --features "$features" \
        --bench "libc_$suite" --message-format=json \
        > "/results/bin/$variant/$suite-build.jsonl"
done
