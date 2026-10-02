#!/usr/bin/bash

set -eu

"$(dirname "$0")/man_create.sh"
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

docker_run_env() {
    local args=()

    for name in "${env_names[@]}"; do
        if [[ -v $name ]]; then
            args+=("-e" "$name=${!name}")
        fi
    done

    docker run --rm -it "${args[@]}" "$@"
}

select_cargo_features() {
    local selection

    if [[ ! -t 0 ]]; then
        echo "Set CARGO_FEATURES for a non-interactive build." >&2
        return 1
    fi

    echo "Select Cargo features:"
    PS3="Selection [1-6]: "
    select selection in \
        "desktop,embed_frontend (default)" \
        "embed_frontend" \
        "desktop,embed_frontend,ffmpeg-filter" \
        "desktop,embed_frontend,ffmpeg-device" \
        "desktop,embed_frontend,ffmpeg-filter,ffmpeg-device" \
        "Custom"; do
        case $REPLY in
            1)
                CARGO_FEATURES="desktop,embed_frontend"
                break
                ;;
            2)
                CARGO_FEATURES="embed_frontend"
                break
                ;;
            3)
                CARGO_FEATURES="desktop,embed_frontend,ffmpeg-filter"
                break
                ;;
            4)
                CARGO_FEATURES="desktop,embed_frontend,ffmpeg-device"
                break
                ;;
            5)
                CARGO_FEATURES="desktop,embed_frontend,ffmpeg-filter,ffmpeg-device"
                break
                ;;
            6)
                read -r -p "Cargo features (comma-separated): " CARGO_FEATURES
                if [[ -n $CARGO_FEATURES ]]; then
                    break
                fi
                echo "Please provide at least one feature."
                ;;
            *)
                echo "Please select a number from 1 to 6."
                ;;
        esac
    done

    if [[ -z ${CARGO_FEATURES:-} ]]; then
        echo "Feature selection cancelled." >&2
        return 1
    fi
}

if [[ -z $target ]]; then
    echo "Pass a target, like: ./scrips/build.sh debian"
    exit 1
fi

case $target in
    debian-shared | debian-static) ;;
    *)
        echo "Unknown target: $target"
        exit 1
        ;;
esac

if [[ ! -v CARGO_FEATURES ]]; then
    select_cargo_features
fi

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < Cargo.toml

echo "Compile ffplayout \"$version\""
echo ""

if [[ $target == "debian-shared" ]]; then
    rm -f ffplayout_${version}-1_amd64.deb
    rm -f "ffplayout-v${version}_debian.tar.gz"

    docker build -t rust-debian -f ./docker/shared.Dockerfile .
    shared_docker_args=(-v "$(pwd)":/src:z)
    if [[ -v CARGO_FEATURES ]]; then
        shared_docker_args+=(-e "CARGO_FEATURES=$CARGO_FEATURES")
    fi
    docker_run_env "${shared_docker_args[@]}" rust-debian

    tar --transform 's/\.\/target\/.*\///g' -czvf "ffplayout-v${version}_debian.tar.gz" --exclude='*.db' --exclude='*.db-shm' \
        --exclude='*.db-wal' assets docker docs LICENSE README.md ./target/release/ffplayout
elif [[ $target == "debian-static" ]]; then
    rm -f ffplayout_${version}-1_amd64.deb
    rm -f ./target/debian-static/ffplayout
    rm -f ./target/debian-static/ffplayout_${version}-1_amd64.deb
    rm -f ./target/release/ffplayout
    mkdir -p ./target/debian-static

    static_cargo_features=$CARGO_FEATURES
    ffmpeg_component_args=()
    if [[ ",$static_cargo_features," == *,ffmpeg-device,* ]]; then
        ffmpeg_component_args+=(--build-arg FFMPEG_AVDEVICE=1)
    fi
    if [[ ",$static_cargo_features," == *,ffmpeg-filter,* ]]; then
        ffmpeg_component_args+=(--build-arg FFMPEG_AVFILTER=1)
    fi

    docker build \
        --build-arg FFMPEG_VERSION="${FFMPEG_VERSION:-release/9.0}" \
        --build-arg FFMPEG_VAAPI="${FFMPEG_VAAPI:-0}" \
        "${ffmpeg_component_args[@]}" \
        --build-arg CARGO_FEATURES="$static_cargo_features" \
        --target static-builder \
        -t localhost/ffplayout-static-builder:latest \
        -f ./docker/static.Dockerfile .

    docker_run_env \
        -v "$(pwd)":/src:z \
        -v "$(pwd)/target/debian-static":/artifacts:z \
        localhost/ffplayout-static-builder:latest

    mv "./target/debian-static/ffplayout_${version}-1_amd64.deb" .
fi
