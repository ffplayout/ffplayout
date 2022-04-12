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
        if [[ -f "ffplayout-rs-v${version}_${target}.zip" ]]; then
            rm -f "ffplayout-rs-v${version}_${target}.zip"
        fi

        cp ./target/${target}/release/ffplayout.exe .
        zip -r "ffplayout-rs-v${version}_${target}.zip" assets docs LICENSE README.md ffplayout.exe
        rm -f ffplayout.exe
    else
        if [[ -f "ffplayout-rs-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-rs-v${version}_${target}.tar.gz"
        fi

        cp ./target/${target}/release/ffplayout .
        tar -czvf "ffplayout-rs-v${version}_${target}.tar.gz" assets docs LICENSE README.md ffplayout
        rm -f ffplayout
    fi

    echo ""
done

echo "Create debian package"
echo ""

cargo deb --target=x86_64-unknown-linux-musl

mv ./target/x86_64-unknown-linux-musl/debian/ffplayout-engine_${version}_amd64.deb .

echo "Create rhel package"
echo ""

cargo generate-rpm --target=x86_64-unknown-linux-musl

mv ./target/x86_64-unknown-linux-musl/generate-rpm/ffplayout-engine-${version}-1.x86_64.rpm .
