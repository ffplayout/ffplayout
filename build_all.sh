#!/usr/bin/bash


targets=("x86_64-unknown-linux-musl" "x86_64-pc-windows-gnu" "x86_64-apple-darwin" "aarch64-apple-darwin")

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < ffplayout-engine/Cargo.toml

echo "Compile ffplayout version is: \"$version\""
echo ""

for target in "${targets[@]}"; do
    echo "compile static for $target"
    echo ""

    if [[ $target == "x86_64-pc-windows-gnu" ]]; then
        if [[ -f "ffplayout-v${version}_${target}.zip" ]]; then
            rm -f "ffplayout-v${version}_${target}.zip"
        fi

        cargo build --release --target=$target

        cp ./target/${target}/release/ffpapi.exe .
        cp ./target/${target}/release/ffplayout.exe .
        zip -r "ffplayout-v${version}_${target}.zip" assets docs LICENSE README.md ffplayout.exe ffpapi.exe -x *.db
        rm -f ffplayout.exe ffpapi.exe
    elif [[ $target == "x86_64-apple-darwin" ]] || [[ $target == "aarch64-apple-darwin" ]]; then
        if [[ -f "ffplayout-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-v${version}_${target}.tar.gz"
        fi

        cargo build --release --target=$target --bin ffplayout

        cp ./target/${target}/release/ffplayout .
        tar -czvf "ffplayout-v${version}_${target}.tar.gz" --exclude='*.db' assets docs LICENSE README.md ffplayout
        rm -f ffplayout
    else
        if [[ -f "ffplayout-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-v${version}_${target}.tar.gz"
        fi

        cargo build --release --target=$target

        cp ./target/${target}/release/ffpapi .
        cp ./target/${target}/release/ffplayout .
        tar -czvf "ffplayout-v${version}_${target}.tar.gz" --exclude='*.db' assets docs LICENSE README.md ffplayout ffpapi
        rm -f ffplayout ffpapi
    fi

    echo ""
done

cargo deb --target=x86_64-unknown-linux-musl -p ffplayout --manifest-path=ffplayout-engine/Cargo.toml -o ffplayout_${version}_amd64.deb

cargo generate-rpm --target=x86_64-unknown-linux-musl -p ffplayout-engine -o ffplayout-${version}-1.x86_64.rpm

