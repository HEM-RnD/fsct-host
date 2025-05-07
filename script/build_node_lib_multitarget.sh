#!/usr/bin/env bash

# Get the directory of the current script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Change directory to the parent of the script's directory
cd "$SCRIPT_DIR/.." || exit

cargo about generate -c about.toml -m ports/node/Cargo.toml notice.hbs > ports/node/NOTICE.md
cp LICENSE ports/node/
cp LICENSE-FSCT.md ports/node/

# Build only the `fsct_volumio_service` for aarch64 linux
cross build --target aarch64-unknown-linux-gnu --release -p fsct-node-lib
cp target/aarch64-unknown-linux-gnu/release/libfsct_node_lib.so ports/node/fsct-lib.linux-arm64-gnu.node
cp ports/node/NOTICE.md ports/node/npm/linux-arm64-gnu/
cp LICENSE ports/node/npm/linux-arm64-gnu/
cp LICENSE-FSCT.md ports/node/npm/linux-arm64-gnu/

# Build only the `fsct_volumio_service` for armv7hf linux
#cross build --target armv7-unknown-linux-gnueabihf --release -p fsct-node-lib
#cp target/armv7-unknown-linux-gnueabihf/release/libfsct_node_lib.so ports/node/fsct-lib.linux-armv7-gnueabihf.node
#cp ports/node/NOTICE.md ports/node/npm/linux-armv7-gnueabihf/
#cp LICENSE ports/node/npm/linux-armv7-gnueabihf/
#cp LICENSE-FSCT.md ports/node/npm/linux-armv7-gnueabihf/

# Build only the `fsct_volumio_service` for armhf linux
cross build --target arm-unknown-linux-gnueabihf --release -p fsct-node-lib
cp target/arm-unknown-linux-gnueabihf/release/libfsct_node_lib.so ports/node/fsct-lib.linux-arm-gnueabihf.node
cp ports/node/NOTICE.md ports/node/npm/linux-arm-gnueabihf/
cp LICENSE ports/node/npm/linux-arm-gnueabihf/
cp LICENSE-FSCT.md ports/node/npm/linux-arm-gnueabihf/

# Build only the `fsct_volumio_service` for x86_64 linux
cross build --target x86_64-unknown-linux-gnu --release -p fsct-node-lib
cp target/x86_64-unknown-linux-gnu/release/libfsct_node_lib.so ports/node/fsct-lib.linux-x64-gnu.node
cp ports/node/NOTICE.md ports/node/npm/linux-x64-gnu/
cp LICENSE ports/node/npm/linux-x64-gnu/
cp LICENSE-FSCT.md ports/node/npm/linux-x64-gnu/


napi artifacts -d .
npm publish --skip-gh-release
