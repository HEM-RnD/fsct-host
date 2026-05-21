# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust workspace implementing Ferrum Streaming Control Technology™ (FSCT) — a protocol for controlling FSCT-compatible USB audio devices. The system is split into a privileged driver process and user-space clients.

## Workspace Members

| Crate | Package | Role |
|-------|---------|------|
| `core/` | `fsct-core` (lib: `fsct`) | Platform-agnostic domain types, protocol definitions, and the `FsctDriver` trait |
| `driver/` | `fsct-driver` | Long-running service/daemon: USB management, player management, IPC server |
| `clients/rust/` | `fsct-client` | Rust IPC client SDK wrapping `FsctDriver` over named pipes / Unix sockets |
| `clients/node/` | `@hemspzoo/fsct-client` | TypeScript IPC client SDK for Node.js applications |

## Build Commands

```bash
# Build everything
cargo build --release

# Build a specific crate
cargo build --package fsct-core --release
cargo build --package fsct-driver --release

# Cross-compile (requires Docker + `cargo install cross`)
cross build --target aarch64-unknown-linux-gnu --release
cross build --target armv7-unknown-linux-gnueabihf --release
cross build --target x86_64-unknown-linux-gnu --release

# Legacy native Node.js bindings
cd bindings/node && npm install && npm run build

# Node.js IPC client
cd clients/node && npm install && npm run build
```

## Testing

```bash
# All workspace tests
cargo test

# Single crate
cargo test --package fsct-core

# Single test by name
cargo test --package fsct-core test_example

# Show stdout
cargo test -- --nocapture
```

Unit tests live in `#[cfg(test)]` modules in the source files. Integration tests live in `<crate>/tests/*.rs`.

## Code Style

- Line width: **120 characters** (`rustfmt.toml`)
- Rust edition: **2024**
- Format: `cargo fmt`
- Debug logging: `RUST_LOG=debug cargo run ...`

## Code Quality Rules

- **Short functions** — each function does one thing. If a function body needs a mental "and then…", split it.
- **Avoid deep nesting** — max ~2–3 levels. Prefer early returns, `?`, and extracting helper functions over nested `if`/`match` arms.
- **Readable over clever** — name things after what they mean, not how they work. Code is read far more than written.
- **No over-engineering** — don't add abstraction layers for hypothetical future needs. Solve the problem at hand.
- **Logical file structure** — don't hesitate to create a new file when a module grows large or has a clear independent responsibility. But don't split files so finely that related logic becomes scattered. A good heuristic: one cohesive concept per file.

## IPC API Compatibility

`docs/ipc.md` is the authoritative specification of the IPC protocol. When changing the IPC API:

- **Add, don't break** — new methods, new optional fields, and new enum variants are always allowed.
- **Never remove or rename** existing methods, fields, or enum values, and never change the type or meaning of existing fields. Clients written against an older `docs/ipc.md` must continue to work.
- **Version bump** — increment `FSCT_PROTOCOL_VERSION.minor` for backwards-compatible additions. Increment `major` only for breaking changes (which should be avoided).

## Architecture

### Driver Operational Modes

The `fsct-driver` binary (`driver/src/bin/fsctd.rs`) can run in three modes:

- **Root service** — runs as admin/root. Owns USB device management (DeviceManager + UsbDeviceWatch) and acts as the IPC server, exposing the `FsctDriver` API to IPC clients.
- **User service** — runs as the logged-in user (one instance per user session). Reads player state from the OS (MPRIS on Linux, etc.) and acts as an IPC client, forwarding state to the root service.
- **Standalone** (test only) — runs both USB management and OS player watching in a single process with no IPC. Used for development and testing without a separate service pair.

### Execution Model

```
User Service (per-session, IPC client)
       │  OS player info (MPRIS / Windows / macOS)
       │  msgpack-rpc over Unix socket / Windows named pipe
       ▼
Root Service — IPC Server (driver/src/ipc.rs)  ──►  FsctDriver trait (core/src/driver.rs)
       │
       ├── PlayerManager (driver/src/player_manager.rs)
       │     └── emits PlayerEvent via broadcast channel
       │
       ├── Orchestrator (driver/src/orchestrator/)
       │     └── subscribes to PlayerEvent + DeviceEvent, applies state to devices
       │
       └── DeviceManager + UsbDeviceWatch (driver/src/device_manager.rs, usb_device_watch.rs)
             └── USB layer: FsctDevice → FsctUsbInterface → nusb
```

### Key Design Points

- **`FsctDriver` trait** (`core/src/driver.rs`): central abstraction for player/device operations. Two implementations: `LocalDriver` (in-process, used by the driver binary itself) and `IpcDriver` (client-side, in `clients/rust/`).
- **IPC transport**: Unix domain socket (`/run/fsct/fsct.sock` on Linux, `/var/run/fsct/fsct.sock` on macOS); named pipe (`\\.\pipe\fsct_driver`) on Windows. Protocol: JSON-RPC 2.0 over NDJSON. See [`docs/ipc.md`](docs/ipc.md) for the full protocol specification — this file is the source of truth for the IPC API.
- **PlayerManager** is a pure event source — it stores player state and emits `PlayerEvent` but never touches devices directly. The `Orchestrator` bridges player events to device control.
- **Device identity**: devices are identified by a `ManagedDeviceId` (UUID) calculated deterministically from VID + PID + serial number (`core/src/device_uuid_calculator.rs`).
- **Platform ports**: `driver/src/ports/` contains platform-specific code gated by `cfg(target_os)`. Linux uses MPRIS/D-Bus (`zbus`); Windows uses named pipes + WinAPI; macOS uses LaunchDaemon.

### Core Domain Types (`core/src/definitions.rs`)

- `FsctStatus` — playback state (Stopped, Playing, Paused, Seeking, Buffering, Error, Unknown)
- `FsctTextMetadata` — metadata slot IDs (title, artist, album, genre — current and queue)
- `PlayerState` / `TrackMetadata` — player state snapshot
- `ManagedDeviceId = Uuid`, `ManagedPlayerId = NonZeroU32`
- `FSCT_PROTOCOL_VERSION` — major must match between client and driver on connect

## Packaging & Scripts

Platform installers consume assets from `packages/` and are built by scripts in `script/`:

- `script/build_windows_installer.ps1` → MSI via WiX v6 (`packages/windows/`)
- `script/build_linux_deb.sh` → Debian package via fpm (`packages/linux/`)
- `script/macos_service_package_builder.sh` → notarized pkg (`packages/macos/`)
- `script/inject_version.sh` → writes the CI-computed version into `Cargo.toml` and `clients/node/package.json` before each build.

Cross-compilation Docker files are in `.cross/` and are used automatically by `cross`.

## Versioning

The version in `Cargo.toml` (`[workspace.package].version`) and `clients/node/package.json` is **always `0.0.0-dev`** in the repository. It is a placeholder; the real version is computed by [GitVersion](https://gitversion.net) in CI and injected by `script/inject_version.sh` before every build. Never commit a real version — `release/*` branches do not "own" the version string. Cutting a release means tagging a commit; nothing in the working tree changes.

### Branch → version

| Ref | Computed version | npm dist-tag | GitHub Release |
|---|---|---|---|
| tag `vX.Y.Z` | `X.Y.Z` | `latest` | Release |
| tag `vX.Y.Z-rc.N` | `X.Y.Z-rc.N` | `next` | Pre-Release |
| `release/X.Y.Z` push | `X.Y.Z-beta.<N>` | — | — (artifacts only) |
| `hotfix/X.Y.Z` push | `X.Y.Z-beta.<N>` | — | — (artifacts only) |
| `develop` push | `X.Y.(Z+1)-alpha.<N>` | — | — (artifacts only) |
| `feature/<slug>` push | `X.Y.(Z+1)-alpha-<slug>.<N>` | — | — (artifacts only) |
| pull request | `X.Y.(Z+1)-pr.<PR>.<N>` | — | — (artifacts only) |
| `workflow_dispatch` | as above for the source branch | per `publish_target` input | per `create_release` input |

`<N>` is commits since the version source. `<PR>` is the pull-request number. Configuration lives in `GitVersion.yml`.

### CI structure

- `.github/workflows/ci.yml` — top-level orchestrator (the only file with `push`/`pull_request`/`workflow_dispatch` triggers).
- `.github/workflows/version.yml` — runs GitVersion, exposes outputs (`version`, `cargo_version`, `msi_version`, `npm_tag`, `is_release`, `is_prerelease`, `should_publish_npm`, `should_create_release`).
- `.github/workflows/{linux,windows,macos,node-client}-build.yml` — reusable (`on: workflow_call` only). Each one runs `script/inject_version.sh` first.
- `.github/workflows/publish.yml` — reusable. Creates the GitHub Release and runs `npm publish` from the same artifacts the build jobs produced. Requires `NPM_TOKEN` secret.

Manual publish: trigger `ci.yml` via "Run workflow" with `publish_target` (`dev` / `next` / `latest`) and/or `create_release` set.

## Key Dependencies

- `nusb` — custom HEM fork for USB communication
- `msgpack-rpc` — custom HEM fork for IPC framing
- `tokio` — async runtime ("full" features)
- `zbus` — D-Bus / MPRIS integration (Linux only)
- `thiserror` / `anyhow` — error handling

## IDE
Project is usually developed in JetBrains Clion or RustRover. If you can use `mcp__clion__` or `mcp__rustrover__` commands, prefer them over console.
