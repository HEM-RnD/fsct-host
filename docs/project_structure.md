# FSCT Host Workspace Structure

This document describes how the repository is organized after the current refactor, what each crate and directory is responsible for, and how to build and test the project.

## High‑level overview
- Technology: Rust 2024 workspace with optional Node.js client.
- Execution model: A native driver process (service/daemon) exposes IPC for local clients. Clients (Rust, Node.js, etc.) talk to the driver via IPC.
- Packaging: OS‑specific installers and service manifests live under packages with helper scripts in script.

## Workspace members (Cargo)
- core (crate name: fsct-core; lib name: fsct)
  - Purpose: Core domain library. Models, definitions, helper utilities that are platform‑agnostic.
  - Key areas now: definitions (protocol/status types), player_state, endpoint and utilities like device UUID calculation.
- driver (crate name: fsct-driver)
  - Purpose: Native driver process (service/daemon). Owns Usb device management, player management, orchestration 
    and server‑side handling (IPC). 
  - Structure: standard Rust crate; the binary entrypoint is thin and delegates into library code.
  - Platform integration (system services, installers) is handled externally by scripts and packages.
  - Depends on fsct-core for domain models and protocol version.
  - Depends on fsct-client for IPC client.
- clients/rust (crate name: fsct-client)
  - Purpose: Rust client SDK for applications that communicate with the driver over IPC.
  - Notes: Contains minimal, OS‑specific transport glue inline (modules gated by cfg(unix) / cfg(windows)). Public API focuses on establishing a connection and issuing driver requests.
  - Depends on fsct-core for shared domain types.
- ports/node
  - Purpose: Node.js bindings/client for JavaScript runtimes. Communicates with the driver directly, without IPC. 
    Will be deprecated in favor of IPC client in the future.
  - Build via npm

## Repository directories
- clients/
  - rust/: Rust client SDK crate (fsct-client).
- core/: Core Rust library crate (fsct-core) with platform‑neutral definitions and logic.
- driver/: Driver crate (fsct-driver) – the long‑running service/daemon executable plus its internal library.
- packages/: Top‑level packaging assets for all platforms (moved from ports/native/packages/)
  - windows/: WiX source, icons/artifacts used by the Windows installer.
  - linux/: Debian packaging metadata, systemd unit files, maintainer scripts
  - macos/: LaunchDaemon plist, component/distribution definitions, pre/post‑install scripts
- script/: Cross‑platform build and packaging scripts that consume packages/
  - build_windows_installer.ps1: Builds and signs Windows MSI/bundle using WiX v6. Uses packages\windows.
  - build_linux_deb.sh: Builds a Debian package using fpm. Uses packages/linux.
  - macos_service_package_builder.sh: Builds a notarized macOS installer pkg. Uses packages/macos.
  - build_node_lib_multitarget.sh (if used): Assists Node multi‑target builds.
- ports/
  - node/: Node.js bindings and tests.
- docs/: Documentation (this file and platform notes, architecture proposals, etc.).

