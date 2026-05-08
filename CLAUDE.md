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
| `bindings/node/` | `fsct-node` (napi) | Node.js bindings (built separately with npm) — **deprecated, subject to change** |

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

# Node.js bindings
cd bindings/node && npm install && npm run build
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
- **IPC transport**: Unix domain socket at `/tmp/fsct-driver.sock` on Linux/macOS; named pipe at `\\.\pipe\fsct-driver` on Windows. Protocol: msgpack-rpc.
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

Cross-compilation Docker files are in `.cross/` and are used automatically by `cross`.

## Key Dependencies

- `nusb` — custom HEM fork for USB communication
- `msgpack-rpc` — custom HEM fork for IPC framing
- `tokio` — async runtime ("full" features)
- `zbus` — D-Bus / MPRIS integration (Linux only)
- `thiserror` / `anyhow` — error handling
