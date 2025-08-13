# Refactoring native ports (Windows/macOS) to the new FsctDriver interface

Date: 2025-08-13
Author: FSCT Host Team
Status: Refactor plan (no code changes yet)

## 1. Context and goal
The new host driver trait `FsctDriver` was added in `core/src/driver.rs` and is publicly exported from `core/src/lib.rs` (lines 55–57: `pub use driver::{FsctDriver, LocalDriver};`). The old API related to the `player` module and device/player watchers is marked as deprecated:
- `#[deprecated] pub mod player;` (lib.rs: 20–21)
- `#[deprecated] mod player_watch;` (lib.rs: 23–24)
- `#[deprecated] mod devices_watch;` (lib.rs: 34–35)
- and their re-exports (lib.rs: 42–49), e.g. `run_player_watch`, `run_devices_watch`, `Player`, `NoopPlayerEventListener`.

The goal of this refactor is to migrate native implementations (Windows and macOS) from deprecated symbols to the new `FsctDriver` interface, so that we:
- do not use anything marked as deprecated,
- base data and event flow on `FsctDriver` and `LocalDriver`,
- simplify service startup (orchestrator + USB watch) using `LocalDriver::run()`.

## 2. Scope (high level)
Changes affect native ports in `ports/native/src/windows` and `ports/native/src/macos`:
- Replace imports/usages of `fsct_core::player::{...}` and `fsct_core::{player, Player}` with imports based on `FsctDriver`, `LocalDriver`, `player_state`.
- Remove custom player event channels (`create_player_events_channel`, `PlayerEventsSender`, `PlayerEventsReceiver`) 
  in favor of using `FsctDriver` methods directly.
- Resign from implementing "native player", instead implement OS watcher (e.g., GSMTC, Now Playing) via `FsctDriver`. 
- For the first shot OS watcher registers the native “player” via `driver.register_player(self_id)` and report state 
  via `driver.update_player_state(...)`. In the future OS watcher may register each player individually via `driver.
  register_player(...)` and use preferred player via `driver.set_preferred_player(...)` for player reported by the 
  OS as the preferred one.
- Ignore commands (like play/pause/seek) and device assignments for now.
- Start background services (orchestrator + USB watch) via `LocalDriver::run()`, without calling `run_player_watch` or `run_devices_watch`.

## 3. Identify deprecated usages to eliminate
Example (Windows, ports/native/src/windows/player/mod.rs, based on context):
- Deprecated:
  - `use fsct_core::player::{create_player_events_channel, PlayerError, PlayerEvent, PlayerEventsReceiver, PlayerEventsSender, PlayerInterface};`
  - `use fsct_core::{player, Player};`
- After refactor:
  - `use fsct_core::FsctDriver;`
  - `use fsct_core::player_state::{PlayerState, TrackMetadata};`
  - `use fsct_core::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};`
  - (if needed) `use fsct_core::{ManagedPlayerId, ManagedDeviceId};` — as re-exported from lib.rs.

Similarly for the macOS `player` module.

In the `service` layers (Windows/macOS) ensure that:
- They do not use `run_player_watch` / `run_devices_watch` (deprecated, lib.rs: 42–49).
- They create a `LocalDriver` and call `LocalDriver::run()` (driver.rs: 76–94) to start the orchestrator and USB watch.

## 4. Target API and imports
- Directional interface: `fsct_core::FsctDriver` (trait) + local implementation: `fsct_core::LocalDriver`.
- Player state and metadata: `fsct_core::player_state::{PlayerState, TrackMetadata}`, `fsct_core::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo}`.
- Identifiers: `fsct_core::{ManagedPlayerId, ManagedDeviceId}`.
- Background services startup: `LocalDriver::run() -> MultiServiceHandle`.

## 5. Architecture after the change
- Each platform holds an `Arc<dyn FsctDriver>` (in practice, `Arc<LocalDriver>`).
- The platform “watcher” (e.g., Windows GSMTC, macOS Now Playing) registers itself as a player:
  - `let player_id = driver.register_player(self_id).await?;`
- On OS changes (status, timeline, metadata), it calls:
  - `driver.update_player_state(player_id, new_player_state).await?;`
- Background services (orchestrator + USB watch) are started via `driver.run().await?`, returning a `MultiServiceHandle` for controlled shutdown.

## 6. API migration map (Old -> New)
- Player::register / PlayerInterface::register -> `FsctDriver::register_player(self_id)`
- Player::update_state / PlayerInterface::update_state -> `FsctDriver::update_player_state(player_id, PlayerState)`
- run_player_watch() -> removed; new flow via `LocalDriver::run()` plus platform-native watcher
- run_devices_watch() -> removed; USB watch is started in `LocalDriver::run()` (driver.rs: 85–92)

## 7. Windows-specific changes (GSMTC)
- In `ports\native\src\windows\player\mod.rs`:
  1) Inject `Arc<dyn FsctDriver>` into the GSMTC watcher component.
  2) On startup, create/obtain a `LocalDriver` (e.g., from the `service` layer) and register a player: `register_player("native-windows-gsmtc")`.
  3) On GSMTC events, compute:
     - `FsctStatus` (mapping from `GlobalSystemMediaTransportControlsSessionPlaybackStatus`) — existing helper functions like `get_status`, `get_rate`, `get_texts` can remain; just package results into `PlayerState`.
     - `TimelineInfo`, `TrackMetadata`, `FsctTextMetadata` — types stay the same; only the dispatch path changes (driver-based).
  4) Call `driver.update_player_state(player_id, state).await` instead of sending through a custom channel.
- In `ports\native\src\windows\service\...`:
  - Create `Arc<LocalDriver>`, call `run()`, and pass the driver reference to the GSMTC watcher.
  - Do not use `run_player_watch` / `run_devices_watch`.

Example sketch (illustrative):
```rust
let driver = Arc::new(LocalDriver::with_new_managers());
let services = driver.run().await?; // orchestrator + usb watch
let player_id = driver.register_player("native-windows-gsmtc".into()).await?;

// in GSMTC callbacks
let state: PlayerState = build_state_from_gsmtc(...);
driver.update_player_state(player_id, state).await?;
```

## 8. macOS-specific changes
- Same pattern as Windows:
  - Inject `Arc<dyn FsctDriver>` into the watcher (Now Playing / MediaPlayer framework).
  - Register a player via `register_player("native-macos-nowplaying")`.
  - Map OS events -> `PlayerState` -> `update_player_state`.
  - Start services through `LocalDriver::run()` in the `service` module.

## 9. Service layer (both platforms)
- Startup sequence:
  1) `let driver = Arc::new(LocalDriver::with_new_managers());`
  2) `let handle = driver.run().await?; // MultiServiceHandle`
  3) Initialize the player watcher, pass `driver`, and register the player.
  4) Expose `StopHandle` / `ServiceHandle` per native port service interfaces (without deprecated helpers from core).

## 10. Errors and edge cases
- Player registration may fail — log and retry with backoff.

## 11. Files to modify
- Windows:
  - `ports\native\src\windows\player\mod.rs` (imports, event channel removal -> driver, dispatch via `update_player_state`)
  - `ports\native\src\windows\service\...` (create and run `LocalDriver`, inject into watcher)
- macOS:
  - `ports\native\src\macos\player\mod.rs` (analogous)
  - `ports\native\src\macos\service\...` (analogous)
- (Optional later) Remove deprecated re-exports from `core/src/lib.rs` after native ports are migrated.

## 12. Acceptance criteria
1) No imports or calls from modules marked as deprecated (`player`, `player_watch`, `devices_watch`).
2) Windows and macOS build and run on `FsctDriver`/`LocalDriver`:
   - player registration,
   - state updates via `update_player_state`,
   - event reception via `subscribe_player_events`.
3) Background services started exclusively through `LocalDriver::run()` (orchestrator + USB watch).
4) Workspace unit tests pass (`cargo test`).
5) Functional behavior preserved (status/timeline/metadata mapping); only the transport path changed.

## 13. Rollout plan (steps)
1) Windows/player: replace imports, inject driver, register player, switch dispatch to `update_player_state`, add event subscription. 
2) Windows/service: initialize `LocalDriver`, `run()`, pass driver into the player watcher.
3) macOS/player: same changes as Windows.
4) macOS/service: same changes as Windows.
5) Remove leftovers from custom event channels and Player/PlayerInterface.
6) Build + tests; fix warnings; update docs if needed.
7) (Optional) Clean up deprecated re-exports in core once they’re unused.

## 14. Code references
- core/src/lib.rs: deprecated modules (20–24, 34–35), deprecated re-exports (42–49), driver export (55–57).
- core/src/driver.rs: `FsctDriver` definition (35–52), `LocalDriver::run()` (76–94), method forwarding (98–134).
- ports/native/src/windows/player/mod.rs: currently imports deprecated APIs (per context ~lines 31–35).
- ports/native/src/macos/player/mod.rs: analogous to Windows.

---
Note: This document describes the plan. Implementation will follow in a subsequent MR/commit according to the steps and acceptance criteria above.
