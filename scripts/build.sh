#!/usr/bin/bash

set -eu

source "$(dirname "$0")/man_create.sh"
target=${1:-}
env_file=".env"
env_names=()

if [[ -f $env_file ]]; then
    while IFS='=' read -r name _; do
        [[ $name =~ ^[[:space:]]*# ]] && continue
        [[ -z ${name//[[:space:]]/} ]] && continue

        name=${name%%[[:space:]]*}
        env_names+=("$name")
    done < "$env_file"

    set -a
    source "$env_file"
    set +a
fi

docker_exec_env() {
    local args=()

    for name in "${env_names[@]}"; do
        if [[ -v $name ]]; then
            args+=("-e" "$name=${!name}")
        fi
    done

    docker exec -it "${args[@]}" build-ffplayout "$@"
}

docker_run_env() {
    local args=()

    for name in "${env_names[@]}"; do
        if [[ -v $name ]]; then
            args+=("-e" "$name=${!name}")
        fi
    done

    docker run --rm -it "${args[@]}" "$@"
}

cargo_features_args=()

if [[ -n ${CARGO_FEATURES:-} ]]; then
    cargo_features_args=(--features "$CARGO_FEATURES")
fi

if [[ -z $target ]]; then
    echo "Pass a target, like: ./scrips/build.sh debian"
    exit 1
fi

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < Cargo.toml

echo "Compile ffplayout \"$version\""
echo ""

if [[ $target == "debian" ]]; then
    rm -f ffplayout_${version}-1_amd64.deb
    rm -f "ffplayout-v${version}_debian.tar.gz"

    docker rm -f build-ffplayout >/dev/null 2>&1 || true
    docker build -t rust-debian -f ./docker/debian.Dockerfile .
    docker run -dit --name build-ffplayout -v "$(pwd)":/src:z rust-debian

    docker_exec_env cargo build --release --package ffplayout "${cargo_features_args[@]}"
    docker exec -it build-ffplayout cargo deb --no-build \
        -p ffplayout --manifest-path=/src/backend/app/Cargo.toml \
        -o /src/ffplayout_${version}-1_amd64.deb

    docker stop build-ffplayout
    docker rm build-ffplayout

    tar --transform 's/\.\/target\/.*\///g' -czvf "ffplayout-v${version}_debian.tar.gz" --exclude='*.db' --exclude='*.db-shm' \
        --exclude='*.db-wal' assets docker docs LICENSE README.md ./target/release/ffplayout
elif [[ $target == "debian-static" ]]; then
    rm -f ffplayout_${version}-1_amd64.deb
    rm -f ./target/debian-static/ffplayout
    rm -f ./target/debian-static/ffplayout_${version}-1_amd64.deb
    rm -f ./target/release/ffplayout
    mkdir -p ./target/debian-static

    docker build \
        --build-arg FFMPEG_DEBUG="${FFMPEG_DEBUG:-0}" \
        --build-arg FFMPEG_VERSION="${FFMPEG_VERSION:-release/8.1}" \
        -t localhost/ffplayout-ffmpeg-static:latest \
        -f ./docker/ffmpeg.Dockerfile .

    docker build \
        --build-arg CARGO_FEATURES="desktop,embed_frontend" \
        -t localhost/ffplayout-static-builder:latest \
        -f ./docker/static.Dockerfile .

    docker_run_env \
        -v "$(pwd)":/src:z \
        -v "$(pwd)/target/debian-static":/artifacts:z \
        localhost/ffplayout-static-builder:latest

    cp "./target/debian-static/ffplayout_${version}-1_amd64.deb" .
fi
