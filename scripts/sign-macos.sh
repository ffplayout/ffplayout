#!/usr/bin/env bash

# Never trace certificate passwords or private key material, including with bash -x.
set +x
set -euo pipefail

usage() {
    echo "Usage: bash scripts/sign-macos.sh <check|sign BINARY|notarize ARCHIVE.zip>"
}

require_env() {
    local name
    for name in "$@"; do
        if [[ -z ${!name:-} ]]; then
            echo "Missing macOS signing setting: $name" >&2
            return 1
        fi
    done
}

operation=${1:-}
case "$operation" in
    -h | --help) usage; exit 0 ;;
    check)
        [[ $# == 1 ]] || { usage >&2; exit 1; }
        require_env APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY \
            APPLE_TEAM_ID APPLE_API_KEY_ID APPLE_API_PRIVATE_KEY APPLE_ISSUER_ID
        exit 0
        ;;
    sign | notarize)
        [[ $# == 2 && -f $2 ]] || { usage >&2; exit 1; }
        ;;
    *) usage >&2; exit 1 ;;
esac

if [[ $(uname -s) != Darwin ]]; then
    echo "Signing and notarization require macOS and Xcode command line tools." >&2
    exit 1
fi

if [[ $operation == sign ]]; then
    require_env APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_TEAM_ID
else
    require_env APPLE_API_KEY_ID APPLE_API_PRIVATE_KEY APPLE_ISSUER_ID
fi

work_dir=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ffplayout-signing.XXXXXX")
keychain_created=false
restore_search_list=false
original_keychains=()
keychain="$work_dir/signing.keychain-db"
cleanup() {
    local status=$?
    trap - EXIT
    if [[ $restore_search_list == true ]]; then
        if ! security list-keychains -d user -s ${original_keychains[@]+"${original_keychains[@]}"}; then
            echo "Could not restore the keychain search list." >&2
            if [[ $status == 0 ]]; then status=1; fi
        fi
    fi
    if [[ $keychain_created == true ]]; then
        if ! security delete-keychain "$keychain" >/dev/null 2>&1; then
            echo "Could not unregister temporary signing keychain." >&2
            if [[ $status == 0 ]]; then status=1; fi
        fi
    fi
    if ! rm -rf "$work_dir"; then
        if [[ $status == 0 ]]; then status=1; fi
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
umask 077

if [[ $operation == sign ]]; then
    binary=$2
    entitlements="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.github/macos/entitlements.plist"
    printf '%s' "$APPLE_CERTIFICATE" | base64 --decode > "$work_dir/certificate.p12"
    signing_password=$(openssl rand -hex 32)
    if [[ ${GITHUB_ACTIONS:-} == true ]]; then
        printf '::add-mask::%s\n' "$signing_password"
    fi

    # Save before create-keychain, which may itself update the search list.
    search_list=$(security list-keychains -d user)
    while IFS= read -r existing_keychain; do
        existing_keychain=$(printf '%s\n' "$existing_keychain" | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//')
        if [[ -n $existing_keychain ]]; then
            original_keychains+=("$existing_keychain")
        fi
    done <<< "$search_list"
    restore_search_list=true
    echo "Create temporary signing keychain."
    security create-keychain -p "$signing_password" "$keychain"
    keychain_created=true
    security set-keychain-settings -lut 21600 "$keychain"
    security unlock-keychain -p "$signing_password" "$keychain"
    security list-keychains -d user -s "$keychain" ${original_keychains[@]+"${original_keychains[@]}"}
    echo "Import signing certificate and private key."
    security import "$work_dir/certificate.p12" -k "$keychain" \
        -P "$APPLE_CERTIFICATE_PASSWORD" -f pkcs12 \
        -T /usr/bin/codesign -T /usr/bin/security >/dev/null
    identities=$(security find-identity -v -p codesigning "$keychain")
    identity_pattern='^[[:space:]]*[0-9]+\)[[:space:]]+([[:xdigit:]]{40})[[:space:]]+"(.*)"'
    identity_hash=$(printf '%s' "$APPLE_SIGNING_IDENTITY" | tr '[:lower:]' '[:upper:]')
    identity_count=0
    matching_count=0
    signing_identity=''
    while IFS= read -r identity; do
        if [[ $identity =~ $identity_pattern ]]; then
            fingerprint=${BASH_REMATCH[1]}
            certificate_name=${BASH_REMATCH[2]}
            identity_count=$((identity_count + 1))
            if [[ $certificate_name == *"$APPLE_SIGNING_IDENTITY"* || $fingerprint == "$identity_hash" ]]; then
                signing_identity=$fingerprint
                matching_count=$((matching_count + 1))
            fi
        fi
    done <<< "$identities"
    if [[ $identity_count == 0 ]]; then
        echo "No valid code-signing identity in APPLE_CERTIFICATE. Export a valid Developer ID Application certificate together with its private key as a password-protected .p12, then store its Base64 in APPLE_CERTIFICATE." >&2
        exit 1
    fi
    if [[ $matching_count != 1 ]]; then
        echo "APPLE_SIGNING_IDENTITY matches $matching_count of $identity_count valid imported identities. Set it to the certificate's full Developer ID Application name or SHA-1 fingerprint, as shown by security find-identity -v -p codesigning." >&2
        exit 1
    fi
    echo "Authorize codesign access to the imported private key."
    security set-key-partition-list -S apple-tool:,apple:,codesign: \
        -s -k "$signing_password" "$keychain" >/dev/null

    # --keychain restricts identity lookup; chain lookup also needs the search list.
    echo "Sign macOS binary."
    codesign --force --options runtime --timestamp --keychain "$keychain" \
        --entitlements "$entitlements" --identifier org.ffplayout.ffplayout \
        --sign "$signing_identity" "$binary"
    codesign --verify --strict --verbose=2 "$binary"
    signature=$(codesign --display --verbose=4 "$binary" 2>&1)
    team=$(printf '%s\n' "$signature" | sed -n 's/^TeamIdentifier=//p')
    if [[ $team != "$APPLE_TEAM_ID" ]] || \
        ! printf '%s\n' "$signature" | grep -q '^Authority=Developer ID Application:'; then
        echo "Signing certificate must be Developer ID Application for APPLE_TEAM_ID." >&2
        exit 1
    fi
    echo "Verified Developer ID signature for team $team."
else
    archive=$2
    private_key="$work_dir/AuthKey.p8"
    case "$APPLE_API_PRIVATE_KEY" in
        *'-----BEGIN PRIVATE KEY-----'*) printf '%s\n' "$APPLE_API_PRIVATE_KEY" > "$private_key" ;;
        *) printf '%s' "$APPLE_API_PRIVATE_KEY" | base64 --decode > "$private_key" ;;
    esac
    openssl pkey -in "$private_key" -noout >/dev/null
    authentication=(--key "$private_key" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_ISSUER_ID")
    report="$work_dir/submission.json"
    submit_status=0
    xcrun notarytool submit "$archive" "${authentication[@]}" \
        --wait --output-format json > "$report" || submit_status=$?
    submission_id=$(plutil -extract id raw -o - "$report" 2>/dev/null || true)
    status=$(plutil -extract status raw -o - "$report" 2>/dev/null || true)
    if [[ $submit_status != 0 || $status != Accepted ]]; then
        echo "Notarization failed: ${status:-no status}, submission ${submission_id:-unavailable}." >&2
        if [[ -n $submission_id ]]; then
            xcrun notarytool log "$submission_id" "${authentication[@]}" || true
        fi
        exit 1
    fi
    echo "Notarization accepted: $submission_id."
    # A standalone Mach-O inside ZIP cannot carry a stapled ticket. Gatekeeper
    # retrieves its notarization ticket online; do not modify the accepted ZIP.
fi
