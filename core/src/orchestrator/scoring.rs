use crate::definitions::FsctStatus;

#[derive(PartialEq, Eq, Clone, Copy, Debug, PartialOrd)]
pub enum Assignment {
    /// Player is assigned to a connected device, but it is not this device
    AssignedToOtherDevice,
    /// Player is not assigned to any device nor preferred by OS/user
    Unassigned,
    /// Player is assigned to a processed device
    AssignedToThisDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl From<FsctStatus> for PlaybackStatus {
    fn from(status: FsctStatus) -> Self {
        match status {
            FsctStatus::Playing => Self::Playing,
            FsctStatus::Paused => Self::Paused,
            _ => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSelectionParams {
    pub status: PlaybackStatus,
    pub assignment: Assignment,
    pub is_last_selected: bool,
    pub has_metadata: bool,
}


// so the importance is that:
// * assignment
// * playing status
// * last selected
// * metadata
// * other statuses (pause/stop)


const ASSIGNED_TO_OTHER_DEVICE_SCORE: isize = 0;
const UNASSIGNED_SCORE: isize = 16;
const ASSIGNED_TO_THIS_DEVICE_SCORE: isize = 32;
const PLAYING_SCORE: isize = 8;
const HAS_METADATA_SCORE: isize = 2;
const IS_LAST_SELECTED_SCORE: isize = 4;
const PAUSED_SCORE: isize = 1;
const STOPPED_SCORE: isize = 0;
/// Constant telling us below which score player shouldn't be taken into consideration as a candidate
/// for being selected.
const IGNORE_PLAYER_THRESHOLD: isize = 18;

impl PlaybackStatus {
    fn score(&self) -> isize {
        match self {
            Self::Playing => PLAYING_SCORE,
            Self::Paused => PAUSED_SCORE,
            Self::Stopped => STOPPED_SCORE,
        }
    }
}

impl Assignment {
    fn score(&self) -> isize {
        match self {
            Assignment::AssignedToOtherDevice => ASSIGNED_TO_OTHER_DEVICE_SCORE,
            Assignment::Unassigned => UNASSIGNED_SCORE,
            Assignment::AssignedToThisDevice => ASSIGNED_TO_THIS_DEVICE_SCORE,
        }
    }
}

impl PlayerSelectionParams {
    pub fn score(&self) -> isize {
        let mut score = 0;
        score += self.has_metadata.then_some(HAS_METADATA_SCORE).unwrap_or(0);
        score += self.is_last_selected.then_some(IS_LAST_SELECTED_SCORE).unwrap_or(0);
        score += self.status.score();
        score += self.assignment.score();

        score
    }
}

pub fn is_better_selection(player_score: isize, current_score: isize) -> bool {
    player_score > current_score && player_score >= IGNORE_PLAYER_THRESHOLD
}