pub mod definitions;

mod device_uuid_calculator;
pub mod driver;
mod endpoint;
pub mod player_state;

pub use player_state::{PlayerState, TrackMetadata, TrackMetadataIterator};

// Export driver abstraction
pub use driver::{DeviceChangeEvent, FsctDriver};

// Re-export protocol version types
pub use definitions::{FSCT_PROTOCOL_VERSION, ProtocolVersion};
// Export device management types
pub use definitions::*;
pub use device_uuid_calculator::calculate_uuid;
pub use endpoint::default_endpoint_path;
