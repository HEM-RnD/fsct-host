# Proposed Architecture: Consolidating Player Watch into Player Manager

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

The proposed architecture consolidates Player Watch functionality into Player Manager:

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
│  - Manages player registrations                                 │
│  - Monitors player state changes                                │
│  - Handles player-device assignments                            │
│  - Routes player events to appropriate devices                  │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Device Watch                             │
│  - Device enumeration and discovery                             │
│  - Device initialization                                        │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    FSCT USB Device Drivers                      │
└─────────────────────────────────────────────────────────────────┘
```

## Key Changes

1. **Player Manager absorbs Player Watch functionality**
   - Maintains PlayerState objects (data structures only)
   - Maintains player-device assignments
   - Routes player events to appropriate devices

2. **Player implementations move to User Service**
   - Windows and macOS player implementations reside in User Service
   - User Service monitors OS-level player state changes
   - User Service sends player state updates to Driver Service

3. **Direct communication between Player Manager and Device Watch**
   - Player Manager knows which devices to update
   - No need for intermediate PlayerEventListener

4. **Simplified data flow**
   - Player state changes flow directly to assigned devices
   - Clearer responsibility separation

## Implementation Approach

1. Create a new `player_manager.rs` module that:
   - Implements player registration and management
   - Maintains PlayerState objects (data structures only)
   - Maintains player-device assignments
   - Routes player state updates to appropriate devices

2. Modify Device Watch to:
   - Focus solely on device discovery and initialization
   - Provide device access to Player Manager

3. Move player implementations to User Service:
   - Relocate Windows and macOS player implementations
   - Implement OS-level player monitoring in User Service
   - Create communication channel between User Service and Driver Service

4. Update service initialization to use Player Manager instead of Player Watch