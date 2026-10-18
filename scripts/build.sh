#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
    echo "Usage: ./scripts/build.sh [--profiling] <debian-shared|debian-static>"
    echo "  --profiling  Keep release debug symbols for CPU profiling (including .deb)."
}

target=""
profiling=false
for argument in "$@"; do
    case $argument in
        --profiling) profiling=true ;;
        -h | --help) usage; exit 0 ;;
        -*) echo "Unknown option: $argument" >&2; usage >&2; exit 1 ;;
        *)
            if [[ -n $target ]]; then
                echo "Unexpected argument: $argument" >&2
                usage >&2
                exit 1
            fi
            target=$argument
            ;;
    esac
done
env_file=".env"
env_names=()

if [[ -f $env_file ]]; then
    while IFS= read -r line || [[ -n $line ]]; do
        if [[ $line =~ ^[[:space:]]*(export[[:space:]]+)?([a-zA-Z_][a-zA-Z0-9_]*)= ]]; then
            env_names+=("${BASH_REMATCH[2]}")
        fi
    done < "$env_file"

    set -a
    source "$env_file"
    set +a
fi

# Apply CLI overrides after .env, and forward profile settings to either builder.
if [[ $profiling == true ]]; then
    export CARGO_PROFILE_RELEASE_DEBUG=1
    export CARGO_PROFILE_RELEASE_STRIP=false
fi
for name in CARGO_PROFILE_RELEASE_DEBUG CARGO_PROFILE_RELEASE_STRIP; do
    if [[ -n ${!name+x} ]]; then
        env_names+=("$name")
    fi
done

container_run_env() {
    local args=()

    # Bash 3.2 treats empty arrays as unset under `set -u`.
    for name in ${env_names[@]+"${env_names[@]}"}; do
        if [[ -n ${!name+x} ]]; then
            args+=("-e" "$name=${!name}")
        fi
    done

    "$container_cli" run --platform linux/amd64 --rm -it ${args[@]+"${args[@]}"} "$@"
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
    usage >&2
    exit 1
fi

case $target in
    debian-shared | debian-static) ;;
    *)
        echo "Unknown target: $target"
        exit 1
        ;;
esac

if [[ -n ${CONTAINER_CLI:-} ]]; then
    container_cli=$CONTAINER_CLI
elif command -v docker >/dev/null 2>&1; then
    container_cli=docker
elif command -v podman >/dev/null 2>&1; then
    container_cli=podman
else
    echo "Neither Docker nor Podman was found. Install one or set CONTAINER_CLI." >&2
    exit 1
fi

if ! command -v "$container_cli" >/dev/null 2>&1; then
    echo "Container CLI not found: $container_cli" >&2
    exit 1
fi

echo "Using container CLI: $container_cli"

if [[ -z ${CARGO_FEATURES:-} ]]; then
    select_cargo_features
fi

./scripts/man_create.sh

version=""
while IFS="= " read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
        break
    fi
done < Cargo.toml

if [[ -z $version ]]; then
    echo "Could not read the version from Cargo.toml." >&2
    exit 1
fi

echo "Compile ffplayout \"$version\""
echo ""

if [[ $target == "debian-shared" ]]; then
    rm -f "ffplayout_${version}-1_amd64.deb"
    rm -f "ffplayout-v${version}_debian.tar.gz"

    "$container_cli" build --platform linux/amd64 -t rust-debian -f ./docker/shared.Dockerfile .
    shared_docker_args=(-v "$(pwd)":/src:z -e "CARGO_FEATURES=$CARGO_FEATURES")
    container_run_env "${shared_docker_args[@]}" rust-debian

    tar -czvf "ffplayout-v${version}_debian.tar.gz" --exclude='*.db' --exclude='*.db-shm' \
        --exclude='*.db-wal' assets docker docs LICENSE README.md -C ./target/release ffplayout
elif [[ $target == "debian-static" ]]; then
    rm -f "ffplayout_${version}-1_amd64.deb"
    rm -f ./target/debian-static/ffplayout
    rm -f "./target/debian-static/ffplayout_${version}-1_amd64.deb"
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

    "$container_cli" build \
        --platform linux/amd64 \
        --build-arg FFMPEG_VERSION="${FFMPEG_VERSION:-release/9.0}" \
        --build-arg FFMPEG_VAAPI="${FFMPEG_VAAPI:-0}" \
        ${ffmpeg_component_args[@]+"${ffmpeg_component_args[@]}"} \
        --build-arg CARGO_FEATURES="$static_cargo_features" \
        --target static-builder \
        -t localhost/ffplayout-static-builder:latest \
        -f ./docker/static.Dockerfile .

    container_run_env \
        -v "$(pwd)":/src:z \
        -v "$(pwd)/target/debian-static":/artifacts:z \
        localhost/ffplayout-static-builder:latest

    mv "./target/debian-static/ffplayout_${version}-1_amd64.deb" .
fi
