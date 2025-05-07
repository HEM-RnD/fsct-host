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

pub use fsct_core::definitions::TimelineInfo as FsctTimelineInfo;
use fsct_core::definitions::{FsctStatus, FsctTextMetadata};
use std::time::{Duration, SystemTime};

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
    /// Position in seconds from track start
    pub position: f64,
    /// Track duration in seconds
    pub duration: f64,
    /// Playback speed rate. Use 1.0
    pub rate: f64,
}

impl From<TimelineInfo> for FsctTimelineInfo {
    fn from(value: TimelineInfo) -> Self {
        FsctTimelineInfo {
            position: Duration::from_secs_f64(value.position),
            duration: Duration::from_secs_f64(value.duration),
            update_time: SystemTime::now(),
            rate: value.rate,
        }
    }
}

#[napi(string_enum)]
pub enum CurrentTextMetadata {
    Title,
    Author,
    Album,
    Genre,
}

impl From<CurrentTextMetadata> for FsctTextMetadata {
    fn from(value: CurrentTextMetadata) -> Self {
        match value {
            CurrentTextMetadata::Title => FsctTextMetadata::CurrentTitle,
            CurrentTextMetadata::Author => FsctTextMetadata::CurrentAuthor,
            CurrentTextMetadata::Album => FsctTextMetadata::CurrentAlbum,
            CurrentTextMetadata::Genre => FsctTextMetadata::CurrentGenre,
        }
    }
}
