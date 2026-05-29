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

use crate::mono_clock::{instant_from_mono_ns, mono_now_ns, mono_ns_of};
use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::num::NonZeroU32;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// A back-to-back sample of the wall and monotonic clocks, exchanged during the time-sync
/// handshake so a client can bridge its monotonic frame to the driver's
/// (see [`crate::mono_offset_ns`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSync {
    /// Wall-clock time in nanoseconds since the Unix epoch, sampled together with `mono_ns`.
    pub wall_ns: u64,
    /// Monotonic time in nanoseconds since the responder's process [`EPOCH`], sampled together with `wall_ns`.
    pub mono_ns: u64,
}

impl TimeSync {
    /// Sample the wall and monotonic clocks back-to-back in this process's frame.
    pub fn sample_now() -> Self {
        let wall_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mono_ns = mono_now_ns();
        Self { wall_ns, mono_ns }
    }
}

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
    /// Monotonic timestamp of the sample, in nanoseconds since the sender's [`EPOCH`].
    /// When crossing the IPC boundary this is converted into the driver's frame
    /// (see [`mono_offset_ns`]) so the driver can compare it directly with its own clock.
    update_mono_ns: i64,
    duration_ms: u64,
    rate: f64,
}

impl From<TimelineInfo> for TimelineWire {
    fn from(t: TimelineInfo) -> Self {
        Self {
            position_ms: t.position.as_millis() as u64,
            update_mono_ns: mono_ns_of(t.update_time) as i64,
            duration_ms: t.duration.as_millis() as u64,
            rate: t.rate,
        }
    }
}

impl From<TimelineWire> for TimelineInfo {
    fn from(w: TimelineWire) -> Self {
        Self {
            position: Duration::from_millis(w.position_ms),
            update_time: instant_from_mono_ns(w.update_mono_ns.max(0) as u64),
            duration: Duration::from_millis(w.duration_ms),
            rate: w.rate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "TimelineWire", into = "TimelineWire")]
pub struct TimelineInfo {
    pub position: Duration,
    /// Monotonic instant at which `position` was captured (jump-free; see [`EPOCH`]).
    pub update_time: Instant,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono_clock::mono_ns_of;

    #[test]
    fn timeline_wire_roundtrips_monotonic_anchor() {
        let original = TimelineInfo {
            position: Duration::from_millis(30_000),
            update_time: Instant::now(),
            duration: Duration::from_millis(240_000),
            rate: 1.0,
        };
        let wire: TimelineWire = original.clone().into();
        assert_eq!(wire.update_mono_ns, mono_ns_of(original.update_time) as i64);
        let back: TimelineInfo = wire.into();
        assert_eq!(back.position, original.position);
        assert_eq!(back.duration, original.duration);
        assert_eq!(back.rate, original.rate);
        assert_eq!(mono_ns_of(back.update_time), mono_ns_of(original.update_time));
    }
}

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
