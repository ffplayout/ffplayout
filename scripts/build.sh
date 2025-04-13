#!/usr/bin/bash

source $(dirname "$0")/man_create.sh
target=$1

if [[ -n $target ]]; then
    targets=($target)
else
    # x86_64-unknown-linux-musl combined with tokio may slow down or cause problems with async pipes
    # is just an observation due to packet loss/damage in the ffmpeg pipe (engine/player/output/mod.rs -> play())
    # for future investigation:
    # https://news.ycombinator.com/item?id=38616023
    # https://www.tweag.io/blog/2023-08-10-rust-static-link-with-mimalloc/
    targets=("x86_64-unknown-linux-gnu" "aarch64-unknown-linux-gnu" "x86_64-pc-windows-gnu" "x86_64-apple-darwin" "aarch64-apple-darwin")
fi

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < Cargo.toml

echo "Compile ffplayout \"$version\""
echo ""

for target in "${targets[@]}"; do
    echo "compile static for $target"
    echo ""

    if [[ $target == "x86_64-pc-windows-gnu" ]]; then
        if [[ -f "ffplayout-v${version}_${target}.zip" ]]; then
            rm -f "ffplayout-v${version}_${target}.zip"
        fi

        cross build --release --target=$target

        cp ./target/${target}/release/ffplayout.exe .
        zip -r "ffplayout-v${version}_${target}.zip" assets docker docs LICENSE README.md CHANGELOG.md ffplayout.exe -x *.db -x *.db-shm -x *.db-wal -x *.service
        rm -f ffplayout.exe
    else
        if [[ -f "ffplayout-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-v${version}_${target}.tar.gz"
        fi

        cross build --release --target=$target

        tar --transform 's/\.\/target\/.*\///g' -czvf "ffplayout-v${version}_${target}.tar.gz" --exclude='*.db' --exclude='*.db-shm' --exclude='*.db-wal' assets docker docs LICENSE README.md CHANGELOG.md ./target/${target}/release/ffplayout
    fi

    echo ""
done

if [[ "${#targets[@]}" == "5" ]] || [[ $targets == "x86_64-unknown-linux-gnu" ]]; then
    cargo deb --no-build --target=x86_64-unknown-linux-gnu -p ffplayout --manifest-path=engine/Cargo.toml -o ffplayout_${version}-1_amd64.deb
    cargo generate-rpm --target=x86_64-unknown-linux-gnu -p engine -o ffplayout-${version}-1.x86_64.rpm
fi

if [[ "${#targets[@]}" == "5" ]] || [[ $targets == "aarch64-unknown-linux-gnu" ]]; then
    cargo deb --no-build --target=aarch64-unknown-linux-gnu --variant=arm64 -p ffplayout --manifest-path=engine/Cargo.toml -o ffplayout_${version}-1_arm64.deb
fi
