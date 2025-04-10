use std::time::SystemTime;
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::definitions::TimelineInfo as FsctTimelineInfo;

#[napi(string_enum)]
pub enum PlayerStatus {
    /// Playback is currently not active.
    Stopped,
    /// Playback is in progress.
    Playing,
    /// Playback is temporarily halted but can be resumed.
    Paused ,
    /// The playback position is being adjusted, either forward or backward.
    Seeking,
    /// Playback is momentarily halted due to data loading or network conditions.
    Buffering,
    /// An issue occurred, and playback cannot proceed.
    Error,
    /// The playback state could not be determined or is undefined.
    Unknown
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

pub fn get_timeline_info(position: f64,
                        duration: f64,
                        rate: f64) -> Option<FsctTimelineInfo> {
    if !duration.is_nan() && !position.is_nan() && !rate.is_nan() {
        Some(TimelineInfo {
            position,
            duration,
            update_time: SystemTime::now(),
            rate: rate as f32
        })
    } else { None }
}