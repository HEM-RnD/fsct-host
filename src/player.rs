use std::fmt;
use async_trait::async_trait;
use std::sync::Arc;
use crate::definitions::TimelineInfo;

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

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Track {
    pub title: String,
    pub artist: String,
}

#[async_trait]
pub trait PlayerInterface: Send + Sync {
    async fn get_current_track(&self) -> Result<Track, PlaybackError>;
    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlaybackError>;
    async fn is_playing(&self) -> Result<bool, PlaybackError>;

    async fn play(&self) -> Result<(), PlaybackError>;
    async fn pause(&self) -> Result<(), PlaybackError>;
    async fn stop(&self) -> Result<(), PlaybackError>;
    async fn next_track(&self) -> Result<(), PlaybackError>;
    async fn previous_track(&self) -> Result<(), PlaybackError>;
}

#[derive(Clone)]
pub struct Player {
    player_impl: Arc<dyn PlayerInterface + Sync + Send>,
}

impl Player {
    pub fn new(player_impl: Arc<dyn PlayerInterface + Sync + Send>) -> Self {
        Self { player_impl }
    }
}

#[async_trait]
impl PlayerInterface for Player {
    async fn get_current_track(&self) -> Result<Track, PlaybackError> {
        self.player_impl.get_current_track().await
    }
    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlaybackError> {
        self.player_impl.get_timeline_info().await
    }
    async fn is_playing(&self) -> Result<bool, PlaybackError> {
        self.player_impl.is_playing().await
    }
    async fn play(&self) -> Result<(), PlaybackError> {
        self.player_impl.play().await
    }
    async fn pause(&self) -> Result<(), PlaybackError> {
        self.player_impl.pause().await
    }
    async fn stop(&self) -> Result<(), PlaybackError> {
        self.player_impl.stop().await
    }
    async fn next_track(&self) -> Result<(), PlaybackError> {
        self.player_impl.next_track().await
    }
    async fn previous_track(&self) -> Result<(), PlaybackError> {
        self.player_impl.previous_track().await
    }
}