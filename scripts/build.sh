#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
    echo "Usage: ./scripts/build.sh [--profiling] <debian-shared|debian-static|alpine-static>"
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

source ./scripts/lib/build-container.sh
trap cleanup_build_container EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

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
    debian-shared | debian-static | alpine-static) ;;
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
    if [[ $target == alpine-static ]]; then
        CARGO_FEATURES=embed_frontend
    else
        select_cargo_features
    fi
fi

if [[ $target == alpine-static ]]; then
    if printf '%s' "$CARGO_FEATURES" | grep -Eq '(^|[,[:space:]])([^,[:space:]]*/)?desktop(-[[:alnum:]_-]+)?([,[:space:]]|$)'; then
        echo "alpine-static supports server outputs only; remove desktop features." >&2
        exit 1
    fi
    if [[ ${FFMPEG_VAAPI:-0} != 0 ]]; then
        echo "alpine-static does not support VAAPI or Intel VPL." >&2
        exit 1
    fi
fi

build_step "Generate manual" ./scripts/man_create.sh

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

source scripts/lib/deb-package.sh
# Both Debian builder images use Trixie, independently of the host OS.
deb_linkage=dynamic

if [[ $target == debian-static ]]; then
    deb_linkage=static
fi

deb_version=$(deb_package_version "$version" deb13 "$deb_linkage")

echo "Compile ffplayout \"$version\""
echo ""

ffmpeg_component_args=()

if [[ ",$CARGO_FEATURES," == *,ffmpeg-device,* ]]; then
    ffmpeg_component_args+=(--build-arg FFMPEG_AVDEVICE=1)
fi

if [[ ",$CARGO_FEATURES," == *,ffmpeg-filter,* ]]; then
    ffmpeg_component_args+=(--build-arg FFMPEG_AVFILTER=1)
fi

if [[ $target == "debian-shared" ]]; then
    rm -f "ffplayout_${deb_version}_amd64.deb"
    rm -f "ffplayout-v${version}_debian.tar.gz"

    build_step "Prepare Debian shared builder image (cache check/build)" \
        "$container_cli" build --platform linux/amd64 --build-arg CARGO_FEATURES="$CARGO_FEATURES" \
        --build-arg FFMPEG_VAAPI="${FFMPEG_VAAPI:-0}" \
        ${ffmpeg_component_args[@]+"${ffmpeg_component_args[@]}"} \
        -t rust-debian -f ./docker/shared.Dockerfile .
    shared_docker_args=(-v "$(pwd)":/src:z)
    container_run_env rust-debian "${shared_docker_args[@]}"

    tar -czvf "ffplayout-v${version}_debian.tar.gz" --exclude='*.db' --exclude='*.db-shm' \
        --exclude='*.db-wal' assets docker docs LICENSE README.md -C ./target/release ffplayout
elif [[ $target == "debian-static" || $target == "alpine-static" ]]; then
    static_dockerfile=./docker/static.Dockerfile
    static_image=localhost/ffplayout-static-builder:latest
    static_description="Debian static"
    if [[ $target == alpine-static ]]; then
        static_dockerfile=./docker/musl.Dockerfile
        static_image=localhost/ffplayout-musl-builder:latest
        static_description="Alpine musl static"
        rm -f "ffplayout-v${version}_linux-musl-x86_64.tar.gz"
    else
        rm -f "ffplayout_${deb_version}_amd64.deb"
        rm -f "./target/$target/ffplayout_${deb_version}_amd64.deb"
        rm -f ./target/release/ffplayout
    fi
    rm -f "./target/$target/ffplayout"
    mkdir -p "./target/$target"

    static_cargo_features=$CARGO_FEATURES

    build_step "Prepare $static_description builder image (cache check/build)" "$container_cli" build \
        --platform linux/amd64 \
        --build-arg FFMPEG_VERSION="${FFMPEG_VERSION:-release/9.1}" \
        --build-arg FFMPEG_VAAPI="${FFMPEG_VAAPI:-0}" \
        ${ffmpeg_component_args[@]+"${ffmpeg_component_args[@]}"} \
        --build-arg CARGO_FEATURES="$static_cargo_features" \
        --target static-builder \
        -t "$static_image" \
        -f "$static_dockerfile" .

    container_run_env "$static_image" \
        -v "$(pwd)":/src:z \
        -v "$(pwd)/target/$target":/artifacts:z

    if [[ $target == alpine-static ]]; then
        build_step "Package static musl binary" tar -czf "ffplayout-v${version}_linux-musl-x86_64.tar.gz" \
            --exclude='*.db' --exclude='*.db-shm' --exclude='*.db-wal' \
            assets docker docs migrations LICENSE README.md -C ./target/alpine-static ffplayout
    else
        mv "./target/debian-static/ffplayout_${deb_version}_amd64.deb" .
    fi
fi

if [[ $target == debian-shared || $target == debian-static ]]; then
    build_step "Test Debian package installation" bash scripts/test-deb-install.sh \
        "ffplayout_${deb_version}_amd64.deb" "$container_cli"
fi
