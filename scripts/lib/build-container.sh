# Sourced by build.sh. Keep dependency caches in an owned, stopped container.
active_build_container=""
active_build_lock=""

release_build_lock() {
    if [[ -n $active_build_lock ]]; then
        rm -f "$active_build_lock/pid"
        rmdir "$active_build_lock"
        active_build_lock=""
    fi
}

cleanup_build_container() {
    if [[ -n $active_build_container ]]; then
        "$container_cli" stop --time 5 "$active_build_container" >/dev/null || true
        active_build_container=""
    fi
    release_build_lock
}

container_run_env() {
    local image=$1
    shift
    local workspace workspace_id container created image_id current_image running owner
    local now build_command result name
    local args=()
    workspace=$(pwd -P)
    workspace_id=$(printf '%s' "$workspace" | cksum)
    workspace_id=${workspace_id%% *}
    container="ffplayout-build-${target}-${workspace_id}"
    # mkdir is atomic on both macOS and Linux. Hold ownership through stop so a
    # second invocation cannot inspect/start this same container concurrently.
    mkdir -p "$workspace/target"
    if ! mkdir "$workspace/target/.${container}.lock" 2>/dev/null; then
        echo "Build is locked: $workspace/target/.${container}.lock. Wait for the current build to finish." >&2
        echo "After an unclean exit, remove this lock directory only when no build is running." >&2
        return 1
    fi
    active_build_lock="$workspace/target/.${container}.lock"
    printf '%s\n' "$$" > "$active_build_lock/pid"
    now=$(date +%s)
    image_id=$("$container_cli" image inspect --format '{{.Id}}' "$image")
    build_command=$("$container_cli" image inspect --format '{{index .Config.Cmd 2}}' "$image")
    if [[ -z $build_command || $build_command == '<no value>' ]]; then
        echo "Builder image has no sh -c build command: $image" >&2
        return 1
    fi

    if "$container_cli" container inspect "$container" >/dev/null 2>&1; then
        owner=$("$container_cli" container inspect --format '{{index .Config.Labels "ffplayout.build.workspace"}}' "$container")
        if [[ $owner != "$workspace" ]]; then
            echo "Refusing to use a build container owned by another workspace: $container" >&2
            return 1
        fi
        running=$("$container_cli" container inspect --format '{{.State.Running}}' "$container")
        if [[ $running != false ]]; then
            echo "Build container is already running: $container. Wait for the current build to finish." >&2
            return 1
        fi
        created=$("$container_cli" container inspect --format '{{index .Config.Labels "ffplayout.build.created"}}' "$container")
        current_image=$("$container_cli" container inspect --format '{{.Image}}' "$container")
        if [[ ! $created =~ ^[0-9]+$ ]] || ((now - created > 604800)) || [[ $current_image != "$image_id" ]]; then
            echo "Replacing expired or outdated build container: $container"
            "$container_cli" rm "$container" >/dev/null
        fi
    fi

    if ! "$container_cli" container inspect "$container" >/dev/null 2>&1; then
        echo "Creating build container: $container"
        "$container_cli" create --platform linux/amd64 --name "$container" \
            --label "ffplayout.build.workspace=$workspace" \
            --label "ffplayout.build.created=$now" \
            --entrypoint sh "$@" "$image" -c \
            'trap "exit 0" TERM INT; while :; do sleep 3600 & wait "$!"; done' >/dev/null
    else
        echo "Reusing build container: $container"
    fi

    # Forward fresh values for every build, including profiling/features. Keeping
    # these out of `create` prevents stale settings when the container is reused.
    args=(-i -e "CARGO_FEATURES=$CARGO_FEATURES")
    if [[ -t 0 && -t 1 ]]; then args+=(-t); fi
    for name in ${env_names[@]+"${env_names[@]}"}; do
        if [[ -n ${!name+x} ]]; then
            args+=(-e "$name=${!name}")
        fi
    done

    active_build_container=$container
    "$container_cli" start "$container" >/dev/null
    if "$container_cli" exec "${args[@]}" "$container" sh -c "$build_command"; then
        result=0
    else
        result=$?
    fi
    "$container_cli" stop --time 5 "$container" >/dev/null
    active_build_container=""
    release_build_lock
    return "$result"
}
