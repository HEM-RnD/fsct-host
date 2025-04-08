use std::fmt;
use async_trait::async_trait;
use std::sync::Arc;
use crate::definitions::*;

#[derive(Debug, PartialEq, Clone)]
pub enum PlayerError {
    PermissionDenied,
    FeatureNotSupported,
    UnknownError(String),
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

pub enum PlayerEvent {
    StateChanged(bool),
    TrackChanged(Option<Track>),
    TimelineInfoChanged(Option<TimelineInfo>),
}

pub type PlayerEventListener = futures::channel::mpsc::Receiver<PlayerEvent>;

#[async_trait]
pub trait PlayerInterface: Send + Sync {
    async fn get_current_track(&self) -> Result<Track, PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn is_playing(&self) -> Result<bool, PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }

    async fn play(&self) -> Result<(), PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn pause(&self) -> Result<(), PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn stop(&self) -> Result<(), PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn next_track(&self) -> Result<(), PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn previous_track(&self) -> Result<(), PlayerError>
    {
        Err(PlayerError::FeatureNotSupported)
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventListener, PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
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
    async fn get_current_track(&self) -> Result<Track, PlayerError> {
        self.player_impl.get_current_track().await
    }
    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlayerError> {
        self.player_impl.get_timeline_info().await
    }
    async fn is_playing(&self) -> Result<bool, PlayerError> {
        self.player_impl.is_playing().await
    }
    async fn play(&self) -> Result<(), PlayerError> {
        self.player_impl.play().await
    }
    async fn pause(&self) -> Result<(), PlayerError> {
        self.player_impl.pause().await
    }
    async fn stop(&self) -> Result<(), PlayerError> {
        self.player_impl.stop().await
    }
    async fn next_track(&self) -> Result<(), PlayerError> {
        self.player_impl.next_track().await
    }
    async fn previous_track(&self) -> Result<(), PlayerError> {
        self.player_impl.previous_track().await
    }
}