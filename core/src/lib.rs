pub mod definitions;

pub mod driver;
pub mod player_state;
mod device_uuid_calculator;
mod endpoint;

pub use player_state::PlayerState;

// Export driver abstraction
pub use driver::FsctDriver;

// Re-export protocol version types
pub use definitions::{ProtocolVersion, FSCT_PROTOCOL_VERSION};
// Export device management types
pub use definitions::DeviceInfo;
pub use definitions::ManagedDeviceId;
pub use definitions::ManagedPlayerId;
pub use endpoint::default_endpoint_path;
pub use device_uuid_calculator::calculate_uuid;