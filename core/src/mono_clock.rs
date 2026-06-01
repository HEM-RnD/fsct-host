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

//! Monotonic-clock helpers and the cross-process frame-bridging math used by the time-sync
//! handshake. Everything here is jump-free: it never reads the wall-clock except in the explicit
//! bridging step, so NTP steps cannot corrupt playback timelines.

use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

// Rust documents ~100 years as a cross-platform comfortable range for Instant arithmetic.
const MAX_INSTANT_OFFSET_MS: u64 = 100 * 365 * 24 * 60 * 60 * 1_000;

/// Process-global monotonic epoch, captured once on first access.
///
/// Monotonic timestamps are expressed as milliseconds since this epoch, giving a
/// process-local but jump-free time reference. Milliseconds keep every wire value well
/// within 2^53, so JS clients can use plain `number` arithmetic without precision loss.
/// The epoch differs per process (and per boot), so two processes' frames are reconciled
/// with [`mono_offset_ms`].
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Current monotonic time as milliseconds since this process's [`EPOCH`].
pub fn mono_now_ms() -> u64 {
    EPOCH.elapsed().as_millis() as u64
}

/// Monotonic milliseconds since [`EPOCH`] for an arbitrary `Instant` in this process's frame.
pub fn mono_ms_of(instant: Instant) -> u64 {
    instant.saturating_duration_since(*EPOCH).as_millis() as u64
}

/// Reconstruct an `Instant` from a monotonic millisecond stamp in this process's frame.
pub fn instant_from_mono_ms(ms: u64) -> Instant {
    let offset = Duration::from_millis(ms.min(MAX_INSTANT_OFFSET_MS));
    EPOCH.checked_add(offset).unwrap_or(*EPOCH)
}

/// Convert an OS-provided wall-clock timestamp into the monotonic frame.
///
/// The OS reports when playback info was last updated as a wall-clock `SystemTime`. We measure its
/// age against the current wall-clock and subtract that age from `Instant::now()`, yielding a
/// monotonic anchor immune to wall-clock steps. The age is normally a few seconds, well inside any
/// NTP step window, so the result is a faithful monotonic anchor that no longer drifts when the
/// wall-clock jumps. Callers are responsible for turning their platform timestamp (FILETIME, JXA,
/// …) into a `SystemTime` first.
pub fn instant_from_wall(wall: SystemTime) -> Instant {
    let now = Instant::now();
    match SystemTime::now().duration_since(wall) {
        Ok(age) => now.checked_sub(age).unwrap_or(now),
        Err(_) => now,
    }
}

/// NTP/PTP-style offset (ms) that converts a *client-frame* monotonic stamp into the
/// *driver-frame*: `driver_frame = client_frame + offset`.
///
/// The two processes share the same system monotonic clock, so their frames differ only by
/// a constant per boot. We recover that constant by bridging through the shared wall-clock:
/// IPC/transport latency cancels out (the `wall_c - wall_d` term corrects for which wall
/// instant each side sampled), so only a wall-clock step *during* the handshake can corrupt
/// the result — detected by sampling twice and comparing with [`offsets_consistent`].
pub fn mono_offset_ms(wall_d_ms: i64, mono_d_ms: i64, wall_c_ms: i64, mono_c_ms: i64) -> i64 {
    let offset = (mono_d_ms as i128 - mono_c_ms as i128) + (wall_c_ms as i128 - wall_d_ms as i128);
    offset.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Two offset samples agree when their difference is within tolerance, i.e. no wall-clock
/// step happened between the two handshake round-trips.
pub fn offsets_consistent(a_ms: i64, b_ms: i64, tolerance_ms: i64) -> bool {
    tolerance_ms >= 0 && a_ms.abs_diff(b_ms) <= tolerance_ms as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_offset_is_immune_to_symmetric_latency() {
        // Driver and client share the same monotonic clock; here their mono frames differ
        // by a constant 1_000 ms while wall is shared. The recovered offset must equal that
        // constant no matter how much real time elapses between the two samples (latency).
        let frame_const = 1_000i64;
        for latency_ms in [0i64, 1, 10, 250] {
            let wall_d = 5_000_000i64;
            let mono_d = 7_000_000i64;
            let wall_c = wall_d + latency_ms;
            let mono_c = (mono_d + latency_ms) - frame_const;
            let k = mono_offset_ms(wall_d, mono_d, wall_c, mono_c);
            assert_eq!(k, frame_const, "latency {latency_ms}");
        }
    }

    #[test]
    fn mono_offset_detects_wall_step() {
        // A wall step between the two handshake round-trips makes the offsets disagree.
        let k1 = mono_offset_ms(0, 0, 0, 0);
        let k2 = mono_offset_ms(1_000, 0, 0, 0);
        assert!(!offsets_consistent(k1, k2, 5));
        assert!(offsets_consistent(k1, k1, 5));
    }

    #[test]
    fn instant_from_mono_ms_clamps_unrepresentable_values() {
        let instant = instant_from_mono_ms(u64::MAX);
        assert_eq!(mono_ms_of(instant), MAX_INSTANT_OFFSET_MS);
    }

    #[test]
    fn mono_offset_saturates_extreme_inputs() {
        assert_eq!(mono_offset_ms(0, i64::MAX, i64::MAX, i64::MIN), i64::MAX);
        assert_eq!(mono_offset_ms(i64::MAX, i64::MIN, 0, i64::MAX), i64::MIN);
    }

    #[test]
    fn instant_from_wall_anchors_recent_past() {
        // A wall stamp a few seconds in the past must map to roughly that far before now.
        let wall = SystemTime::now() - Duration::from_secs(3);
        let before = Instant::now();
        let anchor = instant_from_wall(wall);
        assert!(anchor <= before, "anchor must not be in the future");
        let age = before.saturating_duration_since(anchor);
        assert!(
            age >= Duration::from_secs(2) && age <= Duration::from_secs(4),
            "age was {age:?}"
        );
    }

    #[test]
    fn instant_from_wall_clamps_future_stamp() {
        // A wall stamp in the future (clock skew) clamps to ~now, never past it.
        let wall = SystemTime::now() + Duration::from_secs(10);
        let before = Instant::now();
        let anchor = instant_from_wall(wall);
        let after = Instant::now();
        assert!(anchor >= before && anchor <= after);
    }

    #[test]
    fn offsets_consistent_handles_extreme_inputs() {
        assert!(!offsets_consistent(i64::MIN, i64::MAX, i64::MAX));
        assert!(!offsets_consistent(0, 0, -1));
    }
}
