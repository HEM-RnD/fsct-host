use std::fmt;
use async_trait::async_trait;
use std::sync::Arc;
use crate::definitions::*;
use crate::definitions::FsctStatus;

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
#[derive(Debug, PartialEq, Clone, Default)]
pub struct TrackMetadata {
    pub title: Option<String>, //CurrentTitle
    pub artist: Option<String>, //CurrentAuthor
    pub album: Option<String>, //CurrentAlbum
    pub genre: Option<String>, //CurrentGenre
    pub year: Option<String>, //CurrentYear
}

pub struct TrackMetadataIterator<'a> {
    metadata: &'a TrackMetadata,
    fsct_text_metadata: FsctTextMetadata,
}

impl<'a> Iterator for TrackMetadataIterator<'a> {
    type Item = (FsctTextMetadata, &'a Option<String>);
    fn next(&mut self) -> Option<(FsctTextMetadata, &'a Option<String>)> {
        match self.fsct_text_metadata {
            FsctTextMetadata::CurrentTitle => {
                self.fsct_text_metadata = FsctTextMetadata::CurrentAuthor;
                Some((FsctTextMetadata::CurrentTitle, &self.metadata.title))
            }
            FsctTextMetadata::CurrentAuthor => {
                self.fsct_text_metadata = FsctTextMetadata::CurrentAlbum;
                Some((FsctTextMetadata::CurrentAuthor, &self.metadata.artist))
            }
            FsctTextMetadata::CurrentAlbum => {
                self.fsct_text_metadata = FsctTextMetadata::CurrentGenre;
                Some((FsctTextMetadata::CurrentAlbum, &self.metadata.album))
            }
            FsctTextMetadata::CurrentGenre => {
                self.fsct_text_metadata = FsctTextMetadata::CurrentYear;
                Some((FsctTextMetadata::CurrentGenre, &self.metadata.genre))
            }
            FsctTextMetadata::CurrentYear => {
                self.fsct_text_metadata = FsctTextMetadata::CurrentTrack; // unused, so causes None in next iteration
                Some((FsctTextMetadata::CurrentYear, &self.metadata.year))
            }
            _ => None,
        }
    }
}

impl TrackMetadata {
    pub fn get_text(&self, text_type: FsctTextMetadata) -> &Option<String> {
        match text_type {
            FsctTextMetadata::CurrentTitle => &self.title,
            FsctTextMetadata::CurrentAuthor => &self.artist,
            FsctTextMetadata::CurrentAlbum => &self.album,
            FsctTextMetadata::CurrentGenre => &self.genre,
            FsctTextMetadata::CurrentYear => &self.year,
            _ => panic!("Unknown text type"),
        }
    }
    pub fn get_mut_text(&mut self, text_type: FsctTextMetadata) -> &mut Option<String> {
        match text_type {
            FsctTextMetadata::CurrentTitle => &mut self.title,
            FsctTextMetadata::CurrentAuthor => &mut self.artist,
            FsctTextMetadata::CurrentAlbum => &mut self.album,
            FsctTextMetadata::CurrentGenre => &mut self.genre,
            FsctTextMetadata::CurrentYear => &mut self.year,
            _ => panic!("Unknown text type"),
        }
    }

    pub fn iter(&self) -> TrackMetadataIterator {
        TrackMetadataIterator {
            metadata: self,
            fsct_text_metadata: FsctTextMetadata::CurrentTitle,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct PlayerState {
    pub status: FsctStatus,
    pub timeline: Option<TimelineInfo>,
    pub texts: TrackMetadata,
}

#[derive(Debug, PartialEq, Clone)]
pub enum PlayerEvent {
    StatusChanged(FsctStatus),
    TextChanged((FsctTextMetadata, Option<String>)),
    TimelineChanged(Option<TimelineInfo>),
}

pub type PlayerEventListener = futures::channel::mpsc::Receiver<PlayerEvent>;

#[async_trait]
pub trait PlayerInterface: Send + Sync {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError>
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
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        self.player_impl.get_current_state().await
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

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventListener, PlayerError> {
        self.player_impl.listen_to_player_notifications().await
    }
}