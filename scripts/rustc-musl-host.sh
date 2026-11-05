#!/bin/sh

# Cargo passes --target for ffplayout/dependencies, but not for host build
# scripts and proc macros. Bindgen's host process must be able to dlopen libclang.
set -eu

compiler=$1
shift

for argument in "$@"; do
    case "$argument" in
        --target|--target=*) exec "$compiler" "$@" ;;
    esac
done

exec "$compiler" "$@" -C target-feature=-crt-static
