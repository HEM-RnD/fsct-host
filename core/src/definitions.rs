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

use std::fmt::Display;
use std::num::NonZeroU32;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

bitflags! {
    #[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
    pub struct FsctFunctionality: u8 {
        const CurrentPlaybackMetadata = 0x01;
        const CurrentPlaybackProgress = 0x02;
        const CurrentPlaybackStatus = 0x04;
        const PlaybackQueueMetadata = 0x08;
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsctTextMetadata {
    #[default]
    CurrentTitle = 0x01,
    CurrentAuthor = 0x02,
    CurrentAlbum = 0x03,
    CurrentGenre = 0x04,
    QueueTitle = 0x31,
    QueueAuthor = 0x32,
    QueueAlbum = 0x33,
    QueueGenre = 0x34,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum FsctImagePixelFormat {
    #[default]
    Rgb565 = 0x01,
    Rgb888 = 0x02,
    Bgr565 = 0x03,
    Bgr888 = 0x04,
    Grayscale4 = 0x05,
    Grayscale8 = 0x06,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FsctTextDirection {
    LeftToRight = 0,
    RightToLeft = 1,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FsctTextEncoding {
    Utf8 = 0,
    Utf16 = 1,
    Ucs2 = 2,
    Utf32 = 3,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimelineWire {
    position_ms: u64,
    update_unix_ms: i64,
    duration_ms: u64,
    rate: f64,
}

impl From<TimelineInfo> for TimelineWire {
    fn from(t: TimelineInfo) -> Self {
        let update_unix_ms = t.update_time
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(|e| -(e.duration().as_millis() as i64));
        Self {
            position_ms: t.position.as_millis() as u64,
            update_unix_ms,
            duration_ms: t.duration.as_millis() as u64,
            rate: t.rate,
        }
    }
}

impl From<TimelineWire> for TimelineInfo {
    fn from(w: TimelineWire) -> Self {
        let update_time = if w.update_unix_ms >= 0 {
            UNIX_EPOCH + Duration::from_millis(w.update_unix_ms as u64)
        } else {
            UNIX_EPOCH - Duration::from_millis((-w.update_unix_ms) as u64)
        };
        Self {
            position: Duration::from_millis(w.position_ms),
            update_time,
            duration: Duration::from_millis(w.duration_ms),
            rate: w.rate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "TimelineWire", into = "TimelineWire")]
pub struct TimelineInfo {
    pub position: Duration,
    pub update_time: SystemTime,
    pub duration: Duration,
    pub rate: f64,
}

/// Represents the various playback states within the Ferrum Streaming Control Technology (FSCT) system.
///
/// This enumeration defines distinct states that describe the current playback status of a media session
/// in FSCT-enabled devices. It facilitates precise communication of playback conditions between a USB-connected
/// device and a host system.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(non_snake_case)]
#[allow(unused)]
pub enum FsctStatus {
    /// Playback is currently not active.
    Stopped = 0x00,
    /// Playback is in progress.
    Playing = 0x01,
    /// Playback is temporarily halted but can be resumed.
    Paused = 0x02,
    /// The playback position is being adjusted, either forward or backward.
    Seeking = 0x03,
    /// Playback is momentarily halted due to data loading or network conditions.
    Buffering = 0x04,
    /// An issue occurred, and playback cannot proceed.
    Error = 0x05,
    /// The playback state could not be determined or is undefined.
    Unknown = 0x0F,
}

impl Default for FsctStatus {
    fn default() -> Self {
        Self::Unknown
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl ProtocolVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

pub const FSCT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

/// Unique identifier for managed devices
pub type ManagedDeviceId = Uuid;
/// Type alias for player ID
pub type ManagedPlayerId = NonZeroU32;

/// Information about a detected FSCT device
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique identifier for the device (UUID computed from VID, PID, serial number)
    pub id: ManagedDeviceId,
    /// Device name/product string
    pub name: Option<String>,
    /// Manufacturer/vendor string
    pub manufacturer: Option<String>,
    /// USB Vendor ID
    pub vendor_id: u16,
    /// USB Product ID
    pub product_id: u16,
    /// Serial number
    pub serial_number: Option<String>,
}