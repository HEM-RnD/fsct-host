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

# API draft

## Driver Service API

The Driver Service exposes the following APIs via local socket/named pipe:

### Device Management
```rust
// Device discovery and enumeration
async fn list_fsct_capable_devices() -> Result<Vec<DeviceInfo>, DriverError>;
```

### Player Registration and Management
```rust
// Player registration
async fn register_player(player_info: PlayerRegistration) -> Result<PlayerId, DriverError>;
async fn unregister_player(player_id: PlayerId) -> Result<(), DriverError>;

// Player-device assignment
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




