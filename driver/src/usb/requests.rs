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

/// Represents the timestamp in device time.
pub type Timestamp = u64;

#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
#[allow(non_snake_case)]
/// Represents the playback progress of an audio track.
///
/// This structure provides information about the playback state of an audio track,
/// including its total duration, current playback position, playback rate,
/// and the timestamp when the playback state was recorded. It allows tracking
/// the real-time status and progress of the audio playback.
pub struct TrackProgressRequestData {
    /// Audio track duration in seconds.
    pub duration: u32,
    /// Position in seconds from the start of playback. Position below 0 means pre-track silence.
    pub position: i32,
    /// Timestamp in device time at which position was captured in milliseconds since device power-on.
    pub timestamp: Timestamp,
    /// Playback rate.
    pub rate: f32,
}

/// Represents the request codes used in Fsct USB communication.
///
/// This enumeration defines specific codes for handling vendor-specific USB requests
/// in the Fsct USB interface. These requests cover enabling features, retrieving
/// timestamps, progress data, status information, metadata related to text/images,
/// and managing playback queue properties.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_snake_case)]
#[allow(unused)]
pub enum FsctRequestCode {
    /// `enable`: wValue lower half word contains FsctEnable enum values.
    Enable = 0x01,
    /// `timestamp`: type: Timestamp (8 bytes) in device time in milliseconds since device power-on.
    Timestamp = 0x02,
    /// `progress`: type: TrackProgressRequestData.
    Progress = 0x03,
    /// `status`: type: FsctStatus.
    Status = 0x04,
    /// `poll`: empty request for ensuring that service is alive i.e. reset devices internal watchdog without sending any data
    Poll = 0x05,
    /// `currentText`: wIndex lower half word contains FsctTextMetadata enum values.
    CurrentText = 0x10,
    /// `currentImage`: image data is provided in the format described in FsctImageMetadataDescriptor; wIndex contains index of image.
    CurrentImage = 0x11,
    /// `queueLength`: wValue contains queue length.
    QueueLength = 0x21,
    /// `queuePosition`: wValue contains queue position.
    QueuePosition = 0x22,
    /// `queueText`: wIndex lower half word contains FsctTextMetadata enum values; wValue contains index in queue.
    QueueText = 0x23,
}


/// Defines the enabling or disabling states for Ferrum Streaming Control Technology (FSCT) USB function.
///
/// This enumeration represents two states, enable or disable, that configure the activation of specific
/// functionalities within the FSCT framework. It is primarily used to toggle features pertinent to USB-connected
/// device interactions in the streaming control environment.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_snake_case)]
#[allow(unused)]
pub enum FsctEnable {
    /// Indicates that the FSCT function is deactivated.
    Disable = 0x00,
    /// Indicates that the FSCT function is activated.
    Enable = 0x01,
}


