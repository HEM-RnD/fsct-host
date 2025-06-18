#!/usr/bin/env bash

# Get the directory of the current script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.." || exit

cargo about generate -c about.toml -m ports/node/Cargo.toml licenses.hbs > ports/node/LICENSE.md
cp NOTICE ports/node/
cp LICENSE-FSCT.md ports/node/

cross build --target aarch64-unknown-linux-gnu --release -p fsct-node-lib
cp target/aarch64-unknown-linux-gnu/release/libfsct_node_lib.so ports/node/fsct-lib.linux-arm64-gnu.node
cp target/aarch64-unknown-linux-gnu/release/libfsct_node_lib.so ports/node/npm/linux-arm64-gnu/fsct-lib.linux-arm64-gnu.node
cp ports/node/LICENSE.md ports/node/npm/linux-arm64-gnu/
cp NOTICE ports/node/npm/linux-arm64-gnu/
cp LICENSE-FSCT.md ports/node/npm/linux-arm64-gnu/

#cross build --target armv7-unknown-linux-gnueabihf --release -p fsct-node-lib
#cp target/armv7-unknown-linux-gnueabihf/release/libfsct_node_lib.so ports/node/fsct-lib.linux-armv7-gnueabihf.node
#cp target/armv7-unknown-linux-gnueabihf/release/libfsct_node_lib.so ports/node/npm/linux-armv7-gnueabihf/fsct-lib.linux-armv7-gnueabihf.node
#cp ports/node/LICENSE.md ports/node/npm/linux-armv7-gnueabihf/
#cp NOTICE ports/node/npm/linux-armv7-gnueabihf/
#cp LICENSE-FSCT.md ports/node/npm/linux-armv7-gnueabihf/

cross build --target arm-unknown-linux-gnueabihf --release -p fsct-node-lib
cp target/arm-unknown-linux-gnueabihf/release/libfsct_node_lib.so ports/node/fsct-lib.linux-arm-gnueabihf.node
cp target/arm-unknown-linux-gnueabihf/release/libfsct_node_lib.so ports/node/npm/linux-arm-gnueabihf/fsct-lib.linux-arm-gnueabihf.node
cp ports/node/LICENSE.md ports/node/npm/linux-arm-gnueabihf/
cp NOTICE ports/node/npm/linux-arm-gnueabihf/
cp LICENSE-FSCT.md ports/node/npm/linux-arm-gnueabihf/

cross build --target x86_64-unknown-linux-gnu --release -p fsct-node-lib
cp target/x86_64-unknown-linux-gnu/release/libfsct_node_lib.so ports/node/fsct-lib.linux-x64-gnu.node
cp target/x86_64-unknown-linux-gnu/release/libfsct_node_lib.so ports/node/npm/linux-x64-gnu/fsct-lib.linux-x64-gnu.node
cp ports/node/LICENSE.md ports/node/npm/linux-x64-gnu/
cp NOTICE ports/node/npm/linux-x64-gnu/
cp LICENSE-FSCT.md ports/node/npm/linux-x64-gnu/

rm -rf ports/node/LICENSE.md
cp LICENSE ports/node/

cd ports/node/

npm publish --skip-gh-release
