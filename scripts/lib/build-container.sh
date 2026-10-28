# Sourced by build.sh. Keep dependency caches in an owned, stopped container.
active_build_container=""
active_build_lock=""

build_step() {
    local description=$1
    shift
    local step_started=$SECONDS step_status
    printf '[build %s] %s\n' "$(date '+%H:%M:%S')" "$description" >&2
    if "$@"; then
        step_status=0
    else
        step_status=$?
    fi
    printf '[build %s] %s: %ss (exit %s)\n' \
        "$(date '+%H:%M:%S')" "$description" "$((SECONDS - step_started))" "$step_status" >&2
    return "$step_status"
}

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
    local workspace workspace_id container image_id current_image running owner
    local build_command result name metadata container_exists=false inspect_started
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
    # Container CLIs may be remote/VM wrappers. Fetch all required metadata in
    # one request per object rather than starting another CLI for every field.
    metadata=$(build_step "Inspect builder image: $image" \
        "$container_cli" image inspect --format '{{.Id}}|{{index .Config.Cmd 2}}' "$image")
    image_id=${metadata%%|*}
    build_command=${metadata#*|}
    if [[ -z $build_command || $build_command == '<no value>' ]]; then
        echo "Builder image has no sh -c build command: $image" >&2
        return 1
    fi

    inspect_started=$SECONDS
    printf '[build %s] Check existing build container: %s\n' "$(date '+%H:%M:%S')" "$container" >&2
    if metadata=$("$container_cli" container inspect --format \
        '{{.Image}}|{{.State.Running}}|{{index .Config.Labels "ffplayout.build.workspace"}}' \
        "$container" 2>/dev/null); then
        container_exists=true
        IFS='|' read -r current_image running owner <<< "$metadata"
    fi
    printf '[build %s] Container check: %ss (exists=%s)\n' \
        "$(date '+%H:%M:%S')" "$((SECONDS - inspect_started))" "$container_exists" >&2

    if [[ $container_exists == true ]]; then
        if [[ $owner != "$workspace" ]]; then
            echo "Refusing to use a build container owned by another workspace: $container" >&2
            return 1
        fi
        if [[ $running != false ]]; then
            echo "Build container is already running: $container. Wait for the current build to finish." >&2
            return 1
        fi
        if [[ $current_image != "$image_id" ]]; then
            echo "Replacing build container because the builder image changed: $container"
            build_step "Remove build container using old image" "$container_cli" rm "$container" >/dev/null
            container_exists=false
        fi
    fi

    if [[ $container_exists == false ]]; then
        echo "Creating build container: $container"
        build_step "Create build container" "$container_cli" create --platform linux/amd64 --name "$container" \
            --label "ffplayout.build.workspace=$workspace" \
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
    build_step "Start build container" "$container_cli" start "$container" >/dev/null
    if build_step "Run build inside container" "$container_cli" exec "${args[@]}" "$container" sh -c "$build_command"; then
        result=0
    else
        result=$?
    fi
    build_step "Stop build container" "$container_cli" stop --time 5 "$container" >/dev/null
    active_build_container=""
    release_build_lock
    return "$result"
}
