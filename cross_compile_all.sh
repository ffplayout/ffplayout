#!/usr/bin/bash


targets=("x86_64-unknown-linux-musl" "x86_64-pc-windows-gnu" "x86_64-apple-darwin" "aarch64-apple-darwin")

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < Cargo.toml

echo "Compile ffplayout-engine version is: \"$version\""
echo ""

for target in "${targets[@]}"; do
    echo "compile static for $target"
    echo ""

    cargo build --release --target=$target

    if [[ $target == "x86_64-pc-windows-gnu" ]]; then
        if [[ -f "ffplayout-engine-v${version}_${target}.zip" ]]; then
            rm -f "ffplayout-engine-v${version}_${target}.zip"
        fi

        cp ./target/${target}/release/ffplayout.exe .
        cp ./target/${target}/release/ffpapi.exe .
        zip -r "ffplayout-engine-v${version}_${target}.zip" assets docs LICENSE README.md ffplayout.exe ffpapi.exe -x *.db
        rm -f ffplayout.exe ffpapi.exe
    else
        if [[ -f "ffplayout-engine-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-engine-v${version}_${target}.tar.gz"
        fi

        cp ./target/${target}/release/ffplayout .
        cp ./target/${target}/release/ffpapi .
        tar -czvf "ffplayout-engine-v${version}_${target}.tar.gz" --exclude='*.db' assets docs LICENSE README.md ffplayout ffpapi
        rm -f ffplayout ffpapi
    fi

    echo ""
done

echo "Create debian package"
echo ""

cargo deb --target=x86_64-unknown-linux-musl
mv ./target/x86_64-unknown-linux-musl/debian/ffplayout-engine_${version}_amd64.deb .

echo ""
echo "Create rhel package"
echo ""

cargo generate-rpm --target=x86_64-unknown-linux-musl
mv ./target/x86_64-unknown-linux-musl/generate-rpm/ffplayout-engine-${version}-1.x86_64.rpm .
