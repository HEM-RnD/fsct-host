use std::sync::Arc;
use async_trait::async_trait;
use std::fmt;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "macos")] 
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;

#[derive(Debug)]
pub enum PlaybackError {
    NoActivePlayback,
    PermissionDenied,
    FeatureNotSupported,
    UnknownError(String),
}

impl fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActivePlayback => write!(f, "No active playback"),
            Self::PermissionDenied => write!(f, "Permission denied"),
            Self::FeatureNotSupported => write!(f, "Feature not supported"),
            Self::UnknownError(e) => write!(f, "Unknown error: {}", e),
        }
    }
}

#[derive(Debug)]
pub struct Track {
    pub title: String,
    pub artist: String,
}

#[derive(Debug)]
pub struct TimelineInfo {
    pub position: f64,         // current position in seconds
    pub update_time: std::time::SystemTime,  // when the position was last updated
    pub duration: Option<f64>, // total duration in seconds
    pub is_playing: bool,      // playback state
}

#[async_trait]
pub trait PlaybackInfoProvider: Send + Sync {
    async fn get_current_track(&self) -> Result<Track, PlaybackError>;
    async fn get_timeline_info(&self) -> Result<TimelineInfo, PlaybackError>;
    async fn is_playing(&self) -> Result<bool, PlaybackError>;
    async fn get_volume(&self) -> Result<u8, PlaybackError>;
}

#[async_trait]
pub trait PlaybackControlProvider: Send + Sync {
    async fn play(&self) -> Result<(), PlaybackError>;
    async fn pause(&self) -> Result<(), PlaybackError>;
    async fn stop(&self) -> Result<(), PlaybackError>;
    async fn next_track(&self) -> Result<(), PlaybackError>;
    async fn previous_track(&self) -> Result<(), PlaybackError>;
    async fn set_volume(&self, volume: u8) -> Result<(), PlaybackError>;
}

pub struct PlatformContext {
    pub info: Arc<dyn PlaybackInfoProvider>,
    pub control: Arc<dyn PlaybackControlProvider>,
}

#[async_trait]
pub trait PlatformBehavior {
    fn get_platform_name(&self) -> &'static str;
    async fn initialize(&self) -> Result<PlatformContext, String>;
    async fn cleanup(&self) -> Result<(), String>;
}

pub fn get_platform() -> Box<dyn PlatformBehavior> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOSPlatform::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform::new())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        panic!("Unsupported platform");
    }
} 