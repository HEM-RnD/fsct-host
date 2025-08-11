# Proposed Architecture: Event-driven Player Manager and Orchestrator

## Current Architecture

The current architecture has these key components:

1. **Player Watch**
   - Monitors player state changes
   - Processes player events
   - Notifies listeners (DevicesPlayerEventApplier)

2. **Device Watch**
   - Manages device connections/disconnections
   - Initializes FSCT-capable devices
   - Applies player state to devices

3. **DevicesPlayerEventApplier**
   - Bridges between Player Watch and Device Watch
   - Implements PlayerEventListener
   - Applies player events to all connected devices

## Proposed Architecture

The proposed architecture consolidates Player Watch functionality into Player Manager as an event source and introduces an Orchestrator that subscribes to events and decides what to send to which devices:

```
┌─────────────────────────────────────────────────────────────────┐
│                     Client Applications                         │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                Transport Layer (Socket/Named Pipe)              │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                          Driver API                             │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Player Manager                           │
│  - Registers players                                            │
│  - Stores player state and assignments                          │
│  - Emits PlayerEvent via broadcast channel                      │
└───────────────────────────────┬─────────────────────────────────┘
                                │ PlayerEvent (broadcast)
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                           Orchestrator                          │
│  - Subscribes to PlayerEvent and Device events                  │
│  - Routing policy (assigned / unassigned)                       │
│  - Uses DeviceControl or PlayerStateApplier to apply to devices │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                         Device Watch                            │
│  - Device enumeration, discovery, init                          │
│  - Emits Device events (connect/disconnect)                     │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    FSCT USB Device Drivers                      │
└─────────────────────────────────────────────────────────────────┘
```

## Key Changes

1. Player Manager absorbs Player Watch functionality, but only as an event source
   - Maintains PlayerState objects and player-device assignments
   - Emits PlayerEvent on lifecycle, assignment, and state changes (no device I/O in Player Manager)

2. Introduce an Orchestrator layer
   - Subscribes to PlayerEvent and Device events
   - Implements routing policy (assigned vs unassigned handling)
   - Applies states to devices via DeviceControl or a PlayerStateApplier worker

3. Device Watch focuses solely on devices
   - Device enumeration, discovery, initialization
   - Emits Device events (connect/disconnect, capability)

4. Simplified and decoupled data flow
   - Player state changes are broadcast; multiple consumers may react
   - Routing logic lives outside the storage component (Player Manager)

## Implementation Approach

1. Implement Player Manager as an event source
   - Provide register/unregister, assign/unassign, and update APIs
   - Maintain player state and assignments only (no direct device I/O)
   - Expose subscribe() -> broadcast::Receiver<PlayerEvent>

2. Implement Orchestrator task(s)
   - Subscribe to PlayerEvent and Device events (from Device Watch)
   - Maintain routing policy for assigned and unassigned devices
   - Apply states via DeviceControl or PlayerStateApplier (e.g., queue/worker)

3. Keep Device Watch focused on device lifecycle
   - Enumerate, initialize devices; emit connect/disconnect events
   - Provide DeviceControl handle(s) to orchestrator/worker

4. Service integration
   - Initialize Player Manager, Device Watch, and Orchestrator
   - Optionally split assigned/unassigned handling into separate tasks if beneficial

5. Optional: User Service player implementations
   - Monitor OS players and send updates to Driver Service when applicable
   - This remains compatible with the event-driven Player Manager design

---

## Separation of Concerns: PlayerStateApplier component

To decouple orchestration from device I/O, use a PlayerStateApplier in the Orchestrator (not in Player Manager). The orchestrator chooses targets and delegates the act of setting values on devices to the applier, which can be implemented in multiple ways:

- DirectDeviceControlApplier: wraps DeviceControl and performs .await calls directly (minimal changes, useful for simple setups).
- Queue/Worker-based applier: exposes a non-blocking API that enqueues commands and processes them in background asynchronous tasks, improving isolation and enabling backpressure.

With this setup, Player Manager remains a pure store/event source and does not need to know DeviceManager internals. ManagedDeviceId (UUID) is the precise device identity.

### Two operational cases
1. Player assigned to a device
   - Orchestrator applies updates only to the assigned device via PlayerStateApplier.apply_to_device(device_id, state).
   - If a device is not supported by a driver, the applier should surface a typed error. Policy: treat as case 2 or ignore with metrics; recommended to degrade to case 2 if a policy toggle is enabled.

2. Player without assigned device
   - Orchestrator selects the currently active player among unassigned players (e.g., last_active, highest_priority, explicit focus). The chosen state is then propagated to all devices with no assigned player, using PlayerStateApplier to apply per-device.
   - Concrete propagation requires the orchestrator to know the set of unassigned devices; this project keeps that logic outside of Player Manager for clarity.

### Synchronization considerations and potential problems
- Contention on Player Manager state: Guarded by a mutex per player and a global players map. Keep lock hold times short; do not perform device I/O while holding locks. The proposed applier ensures device I/O is outside Player Manager locks.
- Out-of-order updates: When using an async worker, ensure per-device ordering. Solutions:
  - Per-device queue or keyed tasks to preserve ordering.
  - Sequence numbers in commands; workers drop stale commands if a newer sequence arrived.
- Backpressure: If devices are slow/unavailable, queues may grow. Apply bounded channels and shedding policies (drop intermediate progress updates, keep latest).
- Device capability/driver mismatch: Applier should classify errors (unsupported vs transient). Policy could fallback to case 2 behavior for unsupported devices.
- Race conditions on assignment changes:
  - If assignment changes while updates are inflight, ordering matters. Strategy: increment a player assignment generation; applier validates generation before applying.
- Consistency across multiple fields:
  - Apply grouped updates atomically per device as much as the protocol allows; otherwise send in a consistent order (status → timeline → texts) and coalesce when possible.
- Startup/resync cases:
  - On device (re)connect, applier can request latest state from Player Manager or a snapshot store to resync.

### Minimal current implementation in repo
- Added PlayerStateApplier trait and a DirectDeviceControlApplier implementation in core/src/player_state_applier.rs.
- Introduced an event-driven PlayerManager that no longer talks to devices directly. Instead, it emits PlayerEvent notifications via a broadcast channel.
- New module core/src/player_events.rs defines PlayerEvent { Registered, Unregistered, Assigned, Unassigned, StateUpdated }.
- PlayerManager now offers subscribe() -> broadcast::Receiver<PlayerEvent> so independent tasks can listen and decide what to send where (e.g., one task for assigned devices, another for unassigned handling if desired).

This prepares the codebase for introducing background workers without further changes to Player Manager and places routing logic outside of the storage component.

## Event-driven orchestration
- PlayerManager responsibilities: store players, manage assignments, emit events.
- Orchestrator responsibilities: subscribe to PlayerEvent and Device events, maintain routing policy, and apply states to devices using DeviceControl or a PlayerStateApplier-based worker.
- Benefits: clear separation of concerns, easier testing, ability to run multiple independent consumers (assigned/unassigned split) if useful.
- Synchronization: ordering per player can be preserved by consumers; device ordering guaranteed by per-device queues if needed, as discussed above.