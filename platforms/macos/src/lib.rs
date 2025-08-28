//! macOS-specific platform crate for FSCT.

#[cfg(target_os = "macos")]
pub mod service;

#[cfg(target_os = "macos")]
pub mod player;

#[cfg(target_os = "macos")]
pub use service::fsct_main;

#[cfg(target_os = "macos")]
pub use player::run_os_watcher;

// On non-macOS targets, this crate exposes no API (placeholder).
