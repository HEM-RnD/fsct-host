//! Windows-specific platform crate for FSCT.
//! This crate provides the same external API surface as the previous ports/native for Windows builds.


#[cfg(target_os = "windows")]
pub mod service;

#[cfg(target_os = "windows")]
pub mod player;

#[cfg(target_os = "windows")]
pub use service::fsct_main;

#[cfg(target_os = "windows")]
pub use player::run_os_watcher;

// On non-Windows targets, this crate exposes no API (placeholder).
