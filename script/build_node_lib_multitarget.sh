#!/usr/bin/env bash

# Get the directory of the current script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.." || exit

cargo about generate -c about.toml -m bindings/node/Cargo.toml licenses.hbs > bindings/node/LICENSE.md
cp NOTICE bindings/node/
cp LICENSE-FSCT.md bindings/node/

cross build --target aarch64-unknown-linux-gnu --release -p fsct-node-lib
cp target/aarch64-unknown-linux-gnu/release/libfsct_node_lib.so bindings/node/fsct-lib.linux-arm64-gnu.node
cp target/aarch64-unknown-linux-gnu/release/libfsct_node_lib.so bindings/node/npm/linux-arm64-gnu/fsct-lib.linux-arm64-gnu.node
cp bindings/node/LICENSE.md bindings/node/npm/linux-arm64-gnu/
cp NOTICE bindings/node/npm/linux-arm64-gnu/
cp LICENSE-FSCT.md bindings/node/npm/linux-arm64-gnu/

#cross build --target armv7-unknown-linux-gnueabihf --release -p fsct-node-lib
#cp target/armv7-unknown-linux-gnueabihf/release/libfsct_node_lib.so bindings/node/fsct-lib.linux-armv7-gnueabihf.node
#cp target/armv7-unknown-linux-gnueabihf/release/libfsct_node_lib.so bindings/node/npm/linux-armv7-gnueabihf/fsct-lib.linux-armv7-gnueabihf.node
#cp bindings/node/LICENSE.md bindings/node/npm/linux-armv7-gnueabihf/
#cp NOTICE bindings/node/npm/linux-armv7-gnueabihf/
#cp LICENSE-FSCT.md bindings/node/npm/linux-armv7-gnueabihf/

cross build --target arm-unknown-linux-gnueabihf --release -p fsct-node-lib
cp target/arm-unknown-linux-gnueabihf/release/libfsct_node_lib.so bindings/node/fsct-lib.linux-arm-gnueabihf.node
cp target/arm-unknown-linux-gnueabihf/release/libfsct_node_lib.so bindings/node/npm/linux-arm-gnueabihf/fsct-lib.linux-arm-gnueabihf.node
cp bindings/node/LICENSE.md bindings/node/npm/linux-arm-gnueabihf/
cp NOTICE bindings/node/npm/linux-arm-gnueabihf/
cp LICENSE-FSCT.md bindings/node/npm/linux-arm-gnueabihf/

cross build --target x86_64-unknown-linux-gnu --release -p fsct-node-lib
cp target/x86_64-unknown-linux-gnu/release/libfsct_node_lib.so bindings/node/fsct-lib.linux-x64-gnu.node
cp target/x86_64-unknown-linux-gnu/release/libfsct_node_lib.so bindings/node/npm/linux-x64-gnu/fsct-lib.linux-x64-gnu.node
cp bindings/node/LICENSE.md bindings/node/npm/linux-x64-gnu/
cp NOTICE bindings/node/npm/linux-x64-gnu/
cp LICENSE-FSCT.md bindings/node/npm/linux-x64-gnu/

rm -rf bindings/node/LICENSE.md
cp LICENSE bindings/node/

cd bindings/node/

npm publish --skip-gh-release
