# Driver and User Service

Core functionality is divided into two parts:
1. Driver Service
2. User Service

Driver is a service running as an admin (root). It is responsible for detecting and controlling fsct-capable usb 
devices. It exposes a set of APIs to user service.

User service is a service running as a regular user. It is responsible for gathering players and played music from 
the OS and sending it to the driver service. 

Driver and user service communicate via local socket or named pipe.

# Custom players support
Additionaly to players supported through the OS, custom players can be added by using the driver service directly
through the socket or named pipe. 

# Driver service responsibilities and operation details
Driver service is responsible for:
1. Detecting and controlling fsct-capable usb devices.
2. Updating device with the current state, metadata (texts) and timeline. 
3. Managing available players. It includes related players deregistration on connected user or custom service shutdown. 
4. Updating players with the list of available devices (TBD)

Each player instance can be registered by the user service or custom player.
Player can be assigned to a device when it is known which device is used for playback, or present general
information about its state. Then every device which doesn't have assigned player should show the player
information.

# User service operation details
User service registers itself with the driver service as one or many players depending on the host OS API abilities.
Then it updates driver service with the current state, metadata (texts) and timeline for the current player 
of the currently logged-in user. Though there can be multiple user services running, each for a different user.

# Custom player service operation details
Custom player service can be implemented in any language. It can use provided libraries/bindings or implement 
driver's protocol directly. Then it should:
1. Register itself with the driver service as a player.
2. Update driver service with the current state, metadata (texts) and timeline for the player.

# Driver Architecture

## Layered Architecture Overview

The FSCT driver follows a layered architecture pattern with clear separation of concerns:

```
┌─────────────────────────────────────────────────────────────────┐
│                     Client Applications                          │
│  (User Services, Custom Players, System Integration Components)  │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                Transport Layer (Socket/Named Pipe)               │
│  Linux/macOS: Unix domain sockets at /tmp/fsct-driver.sock      │
│  Windows: Named pipe at \\.\pipe\fsct-driver                    │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                          Driver API                              │
│  - Device Management APIs                                        │
│  - Player Registration and Management                            │
│  - Player State Updates                                          │
│  - Event Subscription                                            │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Player Manager                            │
│  - Manages player registrations                                  │
│  - Handles player-device assignments                             │
│  - Processes player state updates                                │
│  - (Not fully implemented yet)                                   │
└───────────────┬───────────────────────────────────┬─────────────┘
                │                                   │
                ▼                                   ▼
┌───────────────────────────────┐   ┌───────────────────────────────┐
│        Device Watch           │   │      Player Watch             │
│  - Device enumeration         │◄──┼──► - Player event handling    │
│  - Device discovery           │   │  - State synchronization      │
│  - Device initialization      │   │  - Event propagation          │
│  - Device-player assignment   │   │                               │
└───────────────┬───────────────┘   └───────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────────────────────────┐
│                    FSCT USB Device Drivers                       │
│  - FSCT protocol implementation                                  │
│  - Device communication                                          │
│  - Status and metadata handling                                  │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                           OS API                                 │
│  - USB communication (provided by nusb library)                  │
│  - Platform-specific device access                               │
└─────────────────────────────────────────────────────────────────┘
```

## Communication Channels

### External Communication
- **Client ↔ Driver**: JSON-serialized messages over socket/named pipe
- **Event Notifications**: Asynchronous event notifications from driver to clients

### Internal Communication
- **Player Manager ↔ Device Watch**: Player state updates and device assignments
- **Device Watch ↔ FSCT USB Drivers**: Device initialization and command execution
- **Player Watch ↔ Player Manager**: Player event propagation
- **Player Watch ↔ Device Watch**: Player state synchronization to devices

## Component Responsibilities

### Driver Socket/Pipes Service
- Provides transport layer for client communication
- Handles connection management and message routing
- Enforces access control and security policies

### Driver API
- Exposes structured API for client applications
- Processes API requests and generates responses
- Manages API versioning and compatibility

### Player Manager
- Central coordination point for player registrations
- Maintains player-device assignment mappings
- Routes player state updates to appropriate devices

### Device Watch
- Monitors USB device connections/disconnections
- Initializes FSCT-capable devices when detected
- Applies player state to connected devices
- Manages device lifecycle and resource cleanup

### Player Watch
- Listens for player state changes
- Propagates player events to interested components
- Maintains current player state information

### FSCT USB Device Drivers
- Implements FSCT-specific USB protocol
- Handles device communication and command execution
- Manages device capabilities and feature detection

### OS API (nusb)
- Provides low-level USB communication
- Handles platform-specific device access
- Manages USB hotplug events and device enumeration

# API draft

## Driver API

The Driver Service exposes the following APIs in rust, and via local socket/named pipe for user services:

### Device Management
```rust
// Device discovery and enumeration (TBD. - maybe unnecessary)
async fn list_fsct_capable_devices() -> Result<Vec<DeviceInfo>, DriverError>;
```

### Player Registration and Management
```rust
// Player registration
async fn register_player(player_info: PlayerRegistration) -> Result<PlayerId, DriverError>;
async fn unregister_player(player_id: PlayerId) -> Result<(), DriverError>;

// Player-device assignment (TBD. using DeviceId or some DeviceInfo)
async fn assign_player_to_device(player_id: PlayerId, device_id: DeviceId) -> Result<(), DriverError>;
async fn unassign_player_from_device(player_id: PlayerId, device_id: DeviceId) -> Result<(), DriverError>;
async fn get_device_assignments(device_id: DeviceId) -> Result<Vec<PlayerId>, DriverError>;
```

### Player State Updates
```rust
// State synchronization
async fn update_player_status(player_id: PlayerId, status: FsctStatus) -> Result<(), DriverError>;
async fn update_player_metadata_text(player_id: PlayerId, metadata_id: FsctTextMetadata, text: Option<String>) -> Result<(), DriverError>;
async fn update_player_timeline(player_id: PlayerId, timeline: Option<TimelineInfo>) -> Result<(), DriverError>;

// Batch updates for efficiency
async fn update_player_state(player_id: PlayerId, state: PlayerState) -> Result<(), DriverError>;
```

### Event Subscription (TBD.)
```rust
// Device events
async fn subscribe_to_device_events() -> Result<DeviceEventReceiver, DriverError>;
```

## Data Structures

### Core Types
```rust
pub type PlayerId = u32;
pub type DeviceId = u32;
pub type ConnectionId = u32;

#[derive(Debug, Clone)]
pub struct PlayerRegistration {
    pub name: String,
    pub player_type: PlayerType,
}

#[derive(Debug, Clone)]
pub enum PlayerType {
    CustomPlayer,
    SystemPlayer,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    pub serial_number: String,
}

#[derive(Debug, Clone)]
pub struct PlayerInfo {
    pub player_id: PlayerId,
    pub name: String,
    pub player_type: PlayerType,
    pub assigned_device: Option<DeviceId>,
    pub connection_id: ConnectionId, // connection id of the player service
    pub state: PlayerState,
}
```

### Event Types
```rust
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Connected(DeviceInfo),
    Disconnected(DeviceId),
}

#[derive(Debug, Clone)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Stop,
    NextTrack,
    PreviousTrack,
}
```

## Communication Protocol

### Transport Layer
- **Linux/macOS**: Unix domain sockets at `/tmp/fsct-driver.sock`
- **Windows**: Named pipe at `\\.\pipe\fsct-driver`

### Message Format (TBD.)
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct DriverMessage {
    pub id: MessageId,
    pub timestamp: SystemTime,
    pub payload: MessagePayload,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MessagePayload {
    Request(ApiRequest),
    Response(ApiResponse),
    Event(EventNotification),
    Error(ErrorResponse),
}
```

### Error Handling (TBD.)
```rust
#[derive(Debug, Error)]
pub enum DriverError {
    #[error("Device not found: {0}")]
    DeviceNotFound(DeviceId),
    
    #[error("Player not registered: {0}")]
    PlayerNotRegistered(PlayerId),
    
    #[error("Communication error: {0}")]
    CommunicationError(String),
    
    #[error("Permission denied")]
    PermissionDenied,
    
    #[error("Service unavailable")]
    ServiceUnavailable,
}
```

## Security Considerations

### Access Control
- Driver service runs with elevated privileges (admin/root)
- User services run with user-level privileges
- Custom players require explicit registration and validation
- Device access is mediated through the driver service only




