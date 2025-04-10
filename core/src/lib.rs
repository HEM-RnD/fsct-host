pub mod usb;
pub mod definitions;
pub mod player;

mod player_watch;
mod service_entry;
mod devices_watch;

pub use service_entry::run_service;
pub use player_watch::run_player_watch;
pub use devices_watch::run_devices_watch;
pub use player::Player;
pub use player_watch::NoopPlayerEventListener;

pub use nusb::DeviceId;
pub use devices_watch::DeviceMap;
pub use devices_watch::DevicesPlayerEventApplier;