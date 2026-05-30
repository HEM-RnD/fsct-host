// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

use fsct::definitions::FsctStatus;

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
// * assignment - has precedence overall
// * playing status - has precedence over other statuses
// * last selected, metadata & other statuses (pause/stop):
//   * generally we prefer last-selected players, but
//   * when they not provide metadata and they are stopped,
//   * we use the paused one with metadata
//   * we also prefer stopped with metadata over paused without metadata
//   * because we want to show something.

const ASSIGNED_TO_OTHER_DEVICE_SCORE: isize = 0;
const UNASSIGNED_SCORE: isize = 32;
const ASSIGNED_TO_THIS_DEVICE_SCORE: isize = 64;
const PLAYING_SCORE: isize = 16;
const HAS_METADATA_SCORE: isize = 4;
const IS_LAST_SELECTED_SCORE: isize = 6;
const PAUSED_SCORE: isize = 3;
const STOPPED_SCORE: isize = 0;
/// Constant telling us below which score player shouldn't be taken into consideration as a candidate
/// for being selected.
const IGNORE_PLAYER_THRESHOLD: isize = 36;

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
