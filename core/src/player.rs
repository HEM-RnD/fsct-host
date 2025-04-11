use crate::definitions::FsctStatus;
use crate::definitions::*;
use async_trait::async_trait;
use std::fmt;
use std::slice::Iter;
use std::sync::Arc;

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
    pub title: Option<String>,  //CurrentTitle
    pub artist: Option<String>, //CurrentAuthor
    pub album: Option<String>,  //CurrentAlbum
    pub genre: Option<String>,  //CurrentGenre
    pub year: Option<String>,   //CurrentYear
}

const TRACK_METADATA_IDS: [FsctTextMetadata; 5] = [
    FsctTextMetadata::CurrentTitle,
    FsctTextMetadata::CurrentAuthor,
    FsctTextMetadata::CurrentAlbum,
    FsctTextMetadata::CurrentGenre,
    FsctTextMetadata::CurrentYear,
];

pub struct TrackMetadataIterator<'a> {
    metadata: &'a TrackMetadata,
    id_iterator: Iter<'static, FsctTextMetadata>,
}

impl<'a> Iterator for TrackMetadataIterator<'a> {
    type Item = (FsctTextMetadata, &'a Option<String>);
    fn next(&mut self) -> Option<(FsctTextMetadata, &'a Option<String>)> {
        if let Some(id) = self.id_iterator.next() {
            return Some((*id, self.metadata.get_text(*id)));
        }
        None
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
            id_iterator: TRACK_METADATA_IDS.iter(),
        }
    }

    pub fn iter_id(&self) -> Iter<'static, FsctTextMetadata> {
        TRACK_METADATA_IDS.iter()
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

pub type PlayerEventsReceiver = tokio::sync::broadcast::Receiver<PlayerEvent>;
pub type PlayerEventsSender = tokio::sync::broadcast::Sender<PlayerEvent>;

pub type PlayerEventReceiveError = tokio::sync::broadcast::error::RecvError;
pub type PlayerEventSendError = tokio::sync::broadcast::error::SendError<PlayerEvent>;

const DEFAULT_CAPACITY: usize = 30;

pub fn create_player_events_channel() -> (PlayerEventsSender, PlayerEventsReceiver) {
    tokio::sync::broadcast::channel(DEFAULT_CAPACITY)
}

#[async_trait]
pub trait PlayerInterface: Send + Sync {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn play(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn pause(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn stop(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn next_track(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn previous_track(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
}

#[derive(Clone)]
pub struct Player {
    player_impl: Arc<dyn PlayerInterface + Sync + Send>,
}

impl Player {
    pub fn new<T: PlayerInterface + Sync + Send + 'static>(player_impl: T) -> Self {
        Self {
            player_impl: Arc::new(player_impl),
        }
    }

    pub fn from_arc(player_impl: Arc<dyn PlayerInterface + Sync + Send>) -> Self {
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

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        self.player_impl.listen_to_player_notifications().await
    }
}
