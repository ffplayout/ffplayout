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

deb_package_version() {
    deb_upstream_version=$(printf '%s' "${1#v}" | tr '-' '~')
    deb_distribution=${2:?Missing build distribution suffix}

    printf '%s-1+%s\n' "$deb_upstream_version" "$deb_distribution"
}
