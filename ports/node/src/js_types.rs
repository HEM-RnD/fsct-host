pub use fsct_core::definitions::TimelineInfo as FsctTimelineInfo;
use fsct_core::definitions::{FsctStatus, FsctTextMetadata};
use std::time::SystemTime;

#[napi(string_enum)]
pub enum PlayerStatus {
    /// Playback is currently not active.
    Stopped,
    /// Playback is in progress.
    Playing,
    /// Playback is temporarily halted but can be resumed.
    Paused,
    /// The playback position is being adjusted, either forward or backward.
    Seeking,
    /// Playback is momentarily halted due to data loading or network conditions.
    Buffering,
    /// An issue occurred, and playback cannot proceed.
    Error,
    /// The playback state could not be determined or is undefined.
    Unknown,
}

impl From<PlayerStatus> for FsctStatus {
    fn from(value: PlayerStatus) -> Self {
        match value {
            PlayerStatus::Stopped => FsctStatus::Stopped,
            PlayerStatus::Playing => FsctStatus::Playing,
            PlayerStatus::Paused => FsctStatus::Paused,
            PlayerStatus::Seeking => FsctStatus::Seeking,
            PlayerStatus::Buffering => FsctStatus::Buffering,
            PlayerStatus::Error => FsctStatus::Error,
            PlayerStatus::Unknown => FsctStatus::Unknown,
        }
    }
}

#[napi(object)]
#[derive(Debug, Clone, PartialEq, Copy, Default)]
pub struct TimelineInfo {
    pub position: f64,
    pub duration: f64,
    pub rate: f64,
}

impl From<TimelineInfo> for FsctTimelineInfo {
    fn from(value: TimelineInfo) -> Self {
        FsctTimelineInfo {
            position: value.position,
            duration: value.duration,
            update_time: SystemTime::now(),
            rate: value.rate as f32,
        }
    }
}

#[napi(string_enum)]
pub enum CurrentTextMetadata {
    Title,
    Author,
    Genre,
    Year,
    Track,
    Album,
    Comment,
    Rating,
}

impl From<CurrentTextMetadata> for FsctTextMetadata {
    fn from(value: CurrentTextMetadata) -> Self {
        match value {
            CurrentTextMetadata::Title => FsctTextMetadata::CurrentTitle,
            CurrentTextMetadata::Author => FsctTextMetadata::CurrentAuthor,
            CurrentTextMetadata::Genre => FsctTextMetadata::CurrentGenre,
            CurrentTextMetadata::Year => FsctTextMetadata::CurrentYear,
            CurrentTextMetadata::Track => FsctTextMetadata::CurrentTrack,
            CurrentTextMetadata::Album => FsctTextMetadata::CurrentAlbum,
            CurrentTextMetadata::Comment => FsctTextMetadata::CurrentComment,
            CurrentTextMetadata::Rating => FsctTextMetadata::CurrentRating,
        }
    }
}
