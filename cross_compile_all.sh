#!/usr/bin/bash


targets=("x86_64-unknown-linux-musl" "x86_64-pc-windows-gnu" "x86_64-apple-darwin" "aarch64-apple-darwin")

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < Cargo.toml

echo "Compile ffplayout-rs version is: \"$version\""
echo ""

for target in "${targets[@]}"; do
    echo "compile static for $target"
    echo ""

    cargo build --release --target=$target

    if [[ $target == "x86_64-pc-windows-gnu" ]]; then
        cp ./target/${target}/release/ffplayout.exe .
        zip "ffplayout-rs-v${version}_${target}.zip" assets docs LICENSE README.md ffplayout.exe
        rm -f ffplayout.exe
    else
        cp ./target/${target}/release/ffplayout .
        tar -czvf "ffplayout-rs-v${version}_${target}.tar.gz" assets docs LICENSE README.md ffplayout
        rm -f ffplayout
    fi

    echo ""
done
