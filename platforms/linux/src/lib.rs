//! Linux-specific platform crate for FSCT.

#[cfg(target_os = "linux")]
pub mod service;

#[cfg(target_os = "linux")]
pub mod player;

#[cfg(target_os = "linux")]
pub use service::fsct_main;

#[cfg(target_os = "linux")]
pub use player::run_os_watcher;

// On non-Linux targets, this crate exposes no API (placeholder).
