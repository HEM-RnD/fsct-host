#!/usr/bin/env bash

# Get the directory of the current script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Change directory to the parent of the script's directory
cd "$SCRIPT_DIR/.." || exit


# Build only the `fsct_volumio_service` for aarch64 linux
cross build --target aarch64-unknown-linux-gnu --release --bin fsct_volumio_service

# Build only the `fsct_volumio_service` for armv7hf linux
cross build --target armv7-unknown-linux-gnueabihf --release --bin fsct_volumio_service

# Build only the `fsct_volumio_service` for x86_64 linux
cross build --target x86_64-unknown-linux-gnu --release --bin fsct_volumio_service


# Copy the built executable for aarch64 linux
mkdir -p target/volumio/aarch64 && cp target/aarch64-unknown-linux-gnu/release/fsct_volumio_service target/volumio/aarch64/

# Copy the built executable for armv7hf linux
mkdir -p target/volumio/armv7hf && cp target/armv7-unknown-linux-gnueabihf/release/fsct_volumio_service target/volumio/armv7hf/

# Copy the built executable for x86_64 linux
mkdir -p target/volumio/x86_64 && cp target/x86_64-unknown-linux-gnu/release/fsct_volumio_service target/volumio/x86_64/
