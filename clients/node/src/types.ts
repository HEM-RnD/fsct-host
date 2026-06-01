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

/** UUID string, lowercase hyphenated (e.g. "550e8400-e29b-41d4-a716-446655440000") */
export type DeviceId = string;

/** Non-zero u32 player identifier assigned by the driver on registration */
export type PlayerId = number;

/** Playback state (matches wire snake_case values from FsctStatus) */
export type FsctStatus =
  | 'stopped'
  | 'playing'
  | 'paused'
  | 'seeking'
  | 'buffering'
  | 'error'
  | 'unknown';

/** Metadata slot identifier (matches wire snake_case values from FsctTextMetadata) */
export type FsctTextMetadata =
  | 'current_title'
  | 'current_author'
  | 'current_album'
  | 'current_genre'
  | 'queue_title'
  | 'queue_author'
  | 'queue_album'
  | 'queue_genre';

/** Playback timeline snapshot. Positions/durations in milliseconds; the anchor is monotonic. */
export interface TimelineInfo {
  positionMs: number;
  /**
   * Monotonic timestamp (milliseconds, in this client's `process.hrtime` frame) at which
   * `positionMs` was sampled. Use {@link FsctIpcClient.monoNowMs} for "now". Omit to anchor at
   * "now" (age 0) — appropriate when the source has no timestamp of its own (e.g. Volumio).
   *
   * The client converts this into the driver's monotonic frame (via the connect-time handshake)
   * before sending, so it is immune to wall-clock steps.
   */
  updateMonoMs?: number;
  durationMs: number;
  rate: number;
}

/** Track text metadata fields */
export interface TrackMetadata {
  title: string | null;
  artist: string | null;
  album: string | null;
  genre: string | null;
}

/** Full player state snapshot */
export interface PlayerState {
  status: FsctStatus;
  timeline: TimelineInfo | null;
  texts: TrackMetadata;
}

/** Information about a detected FSCT device */
export interface DeviceInfo {
  id: DeviceId;
  name: string | null;
  manufacturer: string | null;
  vendorId: number;
  productId: number;
  serialNumber: string | null;
}

/** IPC protocol version */
export interface ProtocolVersion {
  major: number;
  minor: number;
}

/** Device presence change notification from the driver */
export type DeviceChangeEvent = {
  event: 'added' | 'removed';
  deviceId: DeviceId;
};
