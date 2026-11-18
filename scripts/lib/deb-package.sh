#!/bin/sh

# Read the build distribution, rather than the machine running the container CLI.
deb_distribution_suffix() (
    # shellcheck source=/dev/null
    . "${1:-/etc/os-release}"

    case "$ID" in
        debian) printf 'deb%s\n' "${VERSION_ID%%.*}" ;;
        ubuntu) printf 'ubuntu%s\n' "$VERSION_ID" ;;
        *) echo "Unsupported Debian package build distribution: $ID" >&2; exit 1 ;;
    esac
)

allocator_from_features() (
    selected=system
    feature_list=$(printf '%s' "$1" | tr '[:space:]' ',')
    for candidate in jemalloc mimalloc; do
        case ",$feature_list," in
            *,"$candidate",* | *,ffplayout/"$candidate",*)
                if [ "$selected" != system ]; then
                    echo "Choose either jemalloc or mimalloc, not both." >&2
                    exit 1
                fi
                selected=$candidate
                ;;
        esac
    done
    printf '%s\n' "$selected"
)

deb_package_version() {
    deb_upstream_version=$(printf '%s' "${1#v}" | tr '-' '~')
    deb_distribution=${2:?Missing build distribution suffix}

    case "${3:-dynamic}" in
        dynamic) ;;
        static) deb_distribution="$deb_distribution.static" ;;
        *) echo "Unsupported FFmpeg linkage mode: $3" >&2; return 1 ;;
    esac

    deb_allocator=$(allocator_from_features "${4:-}") || return 1
    if [ "$deb_allocator" != system ]; then
        deb_distribution="$deb_distribution.$deb_allocator"
    fi

    printf '%s-1+%s\n' "$deb_upstream_version" "$deb_distribution"
}
