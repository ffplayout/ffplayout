#!/usr/bin/bash

source $(dirname "$0")/man_create.sh
target=$1
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

if [[ -n $target ]]; then
    targets=($target)
else
    targets=("x86_64-unknown-linux-musl" "aarch64-unknown-linux-gnu" "x86_64-pc-windows-gnu" "x86_64-apple-darwin" "aarch64-apple-darwin")
fi

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < Cargo.toml

echo "Compile ffplayout \"$version\""
echo ""

for target in "${targets[@]}"; do
    echo "compile static for $target"
    echo ""

    if [[ $target == "debian" ]]; then
        rm -f ffplayout_${version}-1_amd64.deb
        rm -f "ffplayout-v${version}_debian.tar.gz"

        docker build -t rust-debian -f ./docker/debian.Dockerfile .
        docker run -dit --name build-ffplayout -v "$(pwd)":/src:z rust-debian

        docker_exec_env cargo build --release --target=x86_64-unknown-linux-gnu "${cargo_features_args[@]}"
        docker exec -it build-ffplayout cargo deb --no-build \
            --target=x86_64-unknown-linux-gnu \
            -p ffplayout --manifest-path=/src/engine/app/Cargo.toml \
            -o /src/ffplayout_${version}-1_amd64.deb

        docker stop build-ffplayout
        docker rm build-ffplayout

        tar --transform 's/\.\/target\/.*\///g' -czvf "ffplayout-v${version}_debian.tar.gz" --exclude='*.db' --exclude='*.db-shm' \
            --exclude='*.db-wal' assets docker docs LICENSE README.md ./target/x86_64-unknown-linux-gnu/release/ffplayout
    elif [[ $target == "debian-static" ]]; then
        rm -f ffplayout_${version}-1_amd64.deb
        rm -f ./target/debian-static/ffplayout
        rm -f ./target/debian-static/ffplayout_${version}-1_amd64.deb
        rm -f ./target/release/ffplayout
        mkdir -p ./target/debian-static

        docker build \
         --build-arg FFMPEG_DEBUG=1 \
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
    elif [[ $target == "x86_64-pc-windows-gnu" ]]; then
        if [[ -f "ffplayout-v${version}_${target}.zip" ]]; then
            rm -f "ffplayout-v${version}_${target}.zip"
        fi

        cross build --release --target=$target "${cargo_features_args[@]}"

        cp ./target/${target}/release/ffplayout.exe .
        zip -r "ffplayout-v${version}_${target}.zip" assets docker docs LICENSE README.md ffplayout.exe -x *.db -x *.db-shm -x *.db-wal -x *.service
        rm -f ffplayout.exe
    else
        if [[ -f "ffplayout-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-v${version}_${target}.tar.gz"
        fi

        cross build --release --target=$target "${cargo_features_args[@]}"

        tar --transform 's/\.\/target\/.*\///g' -czvf "ffplayout-v${version}_${target}.tar.gz" --exclude='*.db' --exclude='*.db-shm' --exclude='*.db-wal' assets docker docs LICENSE README.md ./target/${target}/release/ffplayout
    fi

    echo ""
done

if [[ "${#targets[@]}" == "5" ]] || [[ $targets == "x86_64-unknown-linux-musl" ]]; then
    cargo deb --no-build --target=x86_64-unknown-linux-musl -p ffplayout --manifest-path=engine/app/Cargo.toml -o ffplayout_${version}-1_amd64.deb
    cargo generate-rpm --target=x86_64-unknown-linux-musl -p ffplayout -o ffplayout-${version}-1.x86_64.rpm
fi

if [[ "${#targets[@]}" == "5" ]] || [[ $targets == "aarch64-unknown-linux-gnu" ]]; then
    cargo deb --no-build --target=aarch64-unknown-linux-gnu --variant=arm64 -p ffplayout --manifest-path=engine/app/Cargo.toml -o ffplayout_${version}-1_arm64.deb
fi
