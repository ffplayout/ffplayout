#!/usr/bin/bash


targets=("x86_64-unknown-linux-musl" "x86_64-pc-windows-gnu" "x86_64-apple-darwin" "aarch64-apple-darwin")

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < ffplayout-engine/Cargo.toml

echo "Compile ffplayout-engine version is: \"$version\""
echo ""

for target in "${targets[@]}"; do
    echo "compile static for $target"
    echo ""

    cargo build --release --target=$target --bin ffplayout

    if [[ $target == "x86_64-pc-windows-gnu" ]]; then
        if [[ -f "ffplayout-engine-v${version}_${target}.zip" ]]; then
            rm -f "ffplayout-engine-v${version}_${target}.zip"
        fi

        cp ./target/${target}/release/ffplayout.exe .
        zip -r "ffplayout-engine-v${version}_${target}.zip" assets docs LICENSE README.md ffplayout.exe -x *.db
        rm -f ffplayout.exe
    elif [[ $target == "x86_64-apple-darwin" ]] || [[ $target == "aarch64-apple-darwin" ]]; then
        if [[ -f "ffplayout-engine-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-engine-v${version}_${target}.tar.gz"
        fi

        cp ./target/${target}/release/ffplayout .
        tar -czvf "ffplayout-engine-v${version}_${target}.tar.gz" --exclude='*.db' assets docs LICENSE README.md ffplayout
        rm -f ffplayout
    else
        if [[ -f "ffplayout-engine-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-engine-v${version}_${target}.tar.gz"
        fi

        cp ./target/${target}/release/ffplayout .
        tar -czvf "ffplayout-engine-v${version}_${target}.tar.gz" --exclude='*.db' assets docs LICENSE README.md ffplayout
        rm -f ffplayout
    fi

    echo ""
done

cargo deb --target=x86_64-unknown-linux-musl -p ffplayout-engine
mv ./target/x86_64-unknown-linux-musl/debian/ffplayout-engine_${version}_amd64.deb .

cargo generate-rpm --target=x86_64-unknown-linux-musl -p ffplayout-engine
mv ./target/x86_64-unknown-linux-musl/generate-rpm/ffplayout-engine-${version}-1.x86_64.rpm .

IFS="= "
while read -r name value; do
    if [[ $name == "version" ]]; then
        version=${value//\"/}
    fi
done < ffplayout-api/Cargo.toml

echo "Compile ffplayout-api version is: \"$version\""
echo ""

for target in "${targets[@]}"; do
    echo "compile static for $target"
    echo ""

    if [[ $target == "x86_64-pc-windows-gnu" ]]; then
        if [[ -f "ffplayout-api-v${version}_${target}.zip" ]]; then
            rm -f "ffplayout-api-v${version}_${target}.zip"
        fi

        cargo build --release --target=$target --bin ffpapi

        cp ./target/${target}/release/ffpapi.exe .
        zip -r "ffplayout-api-v${version}_${target}.zip" assets docs LICENSE README.md ffpapi.exe -x *.db
        rm -f ffpapi.exe
    elif [[ $target == "x86_64-unknown-linux-musl" ]]; then
        if [[ -f "ffplayout-api-v${version}_${target}.tar.gz" ]]; then
            rm -f "ffplayout-api-v${version}_${target}.tar.gz"
        fi

        cargo build --release --target=$target --bin ffpapi

        cp ./target/${target}/release/ffpapi .
        tar -czvf "ffplayout-api-v${version}_${target}.tar.gz" --exclude='*.db' assets docs LICENSE README.md ffpapi
        rm -f ffpapi
    fi

    echo ""
done

cargo deb --target=x86_64-unknown-linux-musl -p ffplayout-api
mv ./target/x86_64-unknown-linux-musl/debian/ffplayout-api_${version}_amd64.deb .

cargo generate-rpm --target=x86_64-unknown-linux-musl -p ffplayout-api
mv ./target/x86_64-unknown-linux-musl/generate-rpm/ffplayout-api-${version}-1.x86_64.rpm .
