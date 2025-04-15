#!/usr/bin/env bash

# Get the directory of the current script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Change directory to the parent of the script's directory
cd "$SCRIPT_DIR/.." || exit


# Build only the `fsct_volumio_service` for aarch64 linux
cross build --target aarch64-unknown-linux-gnu --release -p fsct-node-lib

# Build only the `fsct_volumio_service` for armv7hf linux
cross build --target armv7-unknown-linux-gnueabihf --release -p fsct-node-lib

# Build only the `fsct_volumio_service` for armhf linux
cross build --target arm-unknown-linux-gnueabihf --release -p fsct-node-lib

# Build only the `fsct_volumio_service` for x86_64 linux
cross build --target x86_64-unknown-linux-gnu --release -p fsct-node-lib
