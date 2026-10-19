#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/heap-profile.sh [--output-dir DIRECTORY] -- FFPLAYOUT [ARGS...]

Start ffplayout under Heaptrack on Linux. Requires heaptrack, tee and sha256sum.
Process diagnostics, DEBUG console logging and timestamps are enabled unless
the corresponding CLI logging option was supplied. All remaining arguments
are passed to ffplayout unchanged. Relative application paths keep their meaning.
With no application arguments, the normal default webserver address is retained.

Example:
  scripts/heap-profile.sh --output-dir /tmp/heap-profiles -- \
    ./target/release/ffplayout --db /path/to/test.db --channel 1 --foreground

Stop the test normally (Ctrl+C for a foreground run). The script does not attach
to, stop, or restart existing services. Run with the same user and environment
as your normal playout. Keep the exact binary/debug symbols for later analysis.
EOF
}

output_dir="./heap-profiles"
while (($#)); do
    case "$1" in
        -h | --help) usage; exit 0 ;;
        --output-dir)
            if (($# < 2)) || [[ -z $2 ]]; then
                echo "--output-dir requires a directory." >&2
                exit 2
            fi
            output_dir=$2
            shift 2
            ;;
        --) shift; break ;;
        *) echo "Unknown option: $1 (put ffplayout arguments after --)." >&2; exit 2 ;;
    esac
done

if (($# == 0)); then
    usage >&2
    exit 2
fi
if [[ $(uname -s) != Linux ]]; then
    echo "This script uses Heaptrack on Linux; on macOS use Instruments Allocations." >&2
    exit 2
fi
for dependency in heaptrack tee sha256sum; do
    if ! command -v "$dependency" >/dev/null 2>&1; then
        echo "Missing dependency: $dependency" >&2
        exit 2
    fi
done

binary=$(command -v -- "$1") || {
    echo "Executable not found: $1" >&2
    exit 2
}
if [[ ! -f $binary || ! -x $binary ]]; then
    echo "Not an executable file: $binary" >&2
    exit 2
fi
binary="$(cd -- "$(dirname -- "$binary")" && pwd)/$(basename -- "$binary")"
shift
args=("$@")
if (($# == 0)); then
    # Logging flags would otherwise suppress ffplayout's argumentless default.
    args=(--listen "${LISTEN-127.0.0.1:8787}")
fi

has_level=false
has_console=false
has_timestamp=false
for argument in "$@"; do
    case "$argument" in
        --log-level | --log-level=*) has_level=true ;;
        --log-to-console) has_console=true ;;
        --log-timestamp) has_timestamp=true ;;
    esac
done
if ! $has_level; then args+=(--log-level debug); fi
if ! $has_console; then args+=(--log-to-console); fi
if ! $has_timestamp; then args+=(--log-timestamp); fi

# Profiles and logs may include paths and arguments; create private, unique runs.
playout_umask=$(umask)
umask 077
mkdir -p -- "$output_dir"
output_dir=$(cd -- "$output_dir" && pwd)
report=$(mktemp -d "$output_dir/ffplayout-$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")
printf '%q ' "$binary" "${args[@]}" > "$report/command.txt"
printf '\n' >> "$report/command.txt"
sha256sum -- "$binary" > "$report/binary.sha256"
uname -a > "$report/system.txt"
heaptrack --version > "$report/heaptrack-version.txt" 2>&1
: > "$report/playout.log"
: > "$report/exit-code.txt"
# Keep the recording private through its directory, without changing the
# permissions of HLS segments or other files created by the application.
umask "$playout_umask"
export FFPLAYOUT_PROCESS_DIAGNOSTICS=true

printf 'Recording in: %s\n' "$report"
printf 'The selected ffplayout process runs until it exits or you stop this test.\n'
# The recorder/application handle terminal SIGINT. Keep their log reader alive
# and wait for the recorder's cleanup before collecting its exit status.
trap ':' INT
set +e
heaptrack --record-only --output "$report/heaptrack" "$binary" "${args[@]}" \
    2>&1 | tee -i "$report/playout.log"
statuses=("${PIPESTATUS[@]}")
set -e
trap - INT
printf '%s\n' "${statuses[0]}" > "$report/exit-code.txt"
printf 'Profile and log: %s\n' "$report"
printf 'Open the resulting heaptrack.gz or heaptrack.zst file with heaptrack_gui.\n'

if ((statuses[0] != 0)); then
    exit "${statuses[0]}"
fi
exit "${statuses[1]}"
