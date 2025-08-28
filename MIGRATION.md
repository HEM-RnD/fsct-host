# Migration: ports/native -> platforms/* and ports/node -> bindings/node

This document explains the transition from the legacy `ports` layout to the new structure:

- `core/` (unchanged): single crate `fsct_core` with internal modules `usb`, `ipc`, `utils`, `model`.
- `platforms/`: per-OS crates with simple directory names but fsct-prefixed crate names, e.g.:
  - `platforms/windows` (crate: `fsct-platform-windows`)
  - `platforms/macos` (crate: `fsct-platform-macos`)
  - `platforms/linux` (crate: `fsct-platform-linux`)
- `bindings/`: language bindings (planned); `ports/node` remains for now and will be moved to `bindings/node` in a later step.

## Why split the platform crate?
The previous `ports/native` monolithic crate used heavy `#[cfg(target_os = ...)]` to include OS-specific code. Splitting by OS reduces cfg complexity and yields clearer per-target dependencies and packaging.

## How do we keep conditional compilation behavior?
Cargo handles this at the crate level:
- Each platform crate declares its OS-only dependencies under a target-specific table, e.g. `[target.'cfg(target_os = "windows")'.dependencies]`.
- The source files include a compile-time guard, e.g. `#[cfg(not(target_os = "windows"))] compile_error!(...)`, ensuring the crate is only usable for that OS.
- Build using a matching target triple:
  - Windows: `cargo build -p fsct-platform-windows --target x86_64-pc-windows-msvc`
  - macOS: `cargo build -p fsct-platform-macos --target aarch64-apple-darwin`
  - Linux: `cargo build -p fsct-platform-linux --target x86_64-unknown-linux-gnu`

If you build the entire workspace on a non-matching host, Cargo still resolves these crates, but the compile_error prevents accidental cross-use; build platform crates only with appropriate targets in CI.

## Current status
- New crates are scaffolded with placeholder implementations that print a message. The real code remains in `ports/native` and will be migrated incrementally.
- Workspace members include both legacy `ports/native` and the new `platforms/*` crates to allow parallel development.

## Next steps for migration
1. Move Windows-specific modules from `ports/native/src/windows/*` into `platforms/windows/src/*` and adjust `fsct_main` and `run_os_watcher` re-exports.
2. Repeat for macOS and Linux.
3. Update scripts in `script/` to invoke the correct platform crate binaries per OS.
4. Deprecate `ports/native` once parity is achieved.

## Bindings directory
We will move `ports/node` to `bindings/node` later to avoid breaking the current npm workflows. The package name (`fsct-node-lib`) can stay intact; only the path changes.
