#!/usr/bin/env bash
set -euo pipefail

package=${1:?Usage: test-deb-install.sh PACKAGE [CONTAINER_CLI] [IMAGE]}
container_cli=${2:-docker}
image=${3:-debian:trixie}
package=$(realpath "$package")

"$container_cli" run --rm --security-opt label=disable \
    -v "$package:/tmp/ffplayout.deb:ro" "$image" sh -ec '
        apt-get update
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends /tmp/ffplayout.deb
        /usr/bin/ffplayout --version
    '
