# FSCT Host Workspace Overview

This document explains how the repository is organized after the recent refactor: what each crate and top‑level directory is responsible for, and how to build, test, and package the project.

## High‑level overview
- Technology: Rust 2024 workspace with an optional Node.js client.
- Execution model: A native driver process (service/daemon) exposes IPC for local clients. Clients (Rust, Node.js, etc.) communicate with the driver over IPC.
- Packaging: OS‑specific installers and service manifests live under `packages/` with helper scripts in `script/`.

## Workspace members (Cargo)
- core (package: `fsct-core`, library: `fsct`)
  - Purpose: Core domain library — platform‑agnostic models, definitions, and helper utilities.
  - Key areas: `definitions` (protocol/status types), `player_state`, `endpoint`, and utilities like device UUID calculation.
- driver (package: `fsct-driver`)
  - Purpose: Native driver process (service/daemon). Owns USB device management, player management, orchestration,
    and the server‑side IPC handling.
  - Structure: thin binary entry point that delegates into library code.
  - Platform integration (system services, installers) is handled by scripts and `packages/`.
  - Depends on `fsct-core` for domain models and protocol version.
- clients/rust (package: `fsct-client`)
  - Purpose: Rust client SDK for applications that talk to the driver via IPC.
  - Notes: Contains minimal OS‑specific transport glue inline (modules gated by `cfg(unix)` / `cfg(windows)`).
    Public API focuses on establishing a connection and issuing driver requests.
  - Depends on `fsct-core` for shared domain types.
- bindings/node
  - Purpose: Node.js bindings/client for JavaScript runtimes. Currently maintained as a separate path; planned to use the same IPC contract as the Rust client.
  - Build: `npm install && npm run build` inside `ports/node`.

## Repository directories
- `clients/`
  - `rust/`: Rust client SDK crate (`fsct-client`).
- `core/`: Core Rust library crate (`fsct-core`) with platform‑neutral definitions and logic.
- `driver/`: Driver crate (`fsct-driver`) — the long‑running service/daemon executable plus its internal library.
- `packages/`: Top‑level packaging assets for all platforms (moved from `ports/native/packages/`).
  - `windows/`: WiX source, icons/artifacts used by the Windows installer.
  - `linux/`: Debian packaging metadata, systemd unit files, maintainer scripts.
  - `macos/`: LaunchDaemon plist, component/distribution definitions, pre/post‑install scripts.
- `script/`: Cross‑platform build and packaging scripts that consume `packages/`.
  - `build_windows_installer.ps1`: Builds and signs Windows MSI/bundle using WiX v6. Uses `packages\windows`.
  - `build_linux_deb.sh`: Builds a Debian package using fpm. Uses `packages/linux`.
  - `macos_service_package_builder.sh`: Builds a notarized macOS installer pkg. Uses `packages/macos`.
  - `build_node_lib_multitarget.sh` (if used): Assists Node multi‑target builds.
- `bindings/`
  - `node/`: Node.js bindings and tests.
- `docs/`: Documentation (this file, platform notes, architecture proposals, etc.).
