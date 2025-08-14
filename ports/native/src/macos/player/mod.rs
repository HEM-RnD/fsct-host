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

use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::player_state::{PlayerState, TrackMetadata};
use fsct_core::{FsctDriver, ManagedPlayerId};
use media_remote::{NowPlaying, NowPlayingInfo, NowPlayingJXA, Subscription};
use std::process::Command;
use std::sync::Mutex;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use anyhow::anyhow;

struct NowPlayingWrapper {
    now_playing: NowPlaying,
}

unsafe impl Send for NowPlayingWrapper {}

pub struct MacOSWatcherHandle {
    // Keep the Now Playing instances alive while service runs
    jxa: Option<NowPlayingJXA>,
    native: Option<Mutex<NowPlayingWrapper>>,
    _player_id: ManagedPlayerId,
}

fn get_current_track(now_playing_info: &NowPlayingInfo) -> TrackMetadata {
    let mut texts = TrackMetadata::default();
    texts.title = now_playing_info.title.clone();
    texts.artist = now_playing_info.artist.clone();
    texts.album = now_playing_info.album.clone();
    texts.genre = None;

    texts
}

fn get_timeline_info(now_playing_info: &NowPlayingInfo) -> Option<TimelineInfo> {
    let duration = now_playing_info.duration?;
    let position = now_playing_info.elapsed_time.unwrap_or(0.0);
    let update_time = now_playing_info.info_update_time.unwrap_or(SystemTime::now());
    let is_playing = now_playing_info.is_playing.unwrap_or(false);
    let rate = if is_playing {
        now_playing_info.playback_rate.unwrap_or(0.0)
    } else {
        0.0
    };

    Some(TimelineInfo {
        position: Duration::from_secs_f64(position),
        update_time,
        duration: Duration::from_secs_f64(duration),
        rate: rate as f64,
    })
}

fn get_status(now_playing_info: &NowPlayingInfo) -> FsctStatus {
    match now_playing_info.playback_rate {
        Some(0.0) => FsctStatus::Paused,
        Some(_) => FsctStatus::Playing,
        None => FsctStatus::Stopped,
    }
}

fn build_state(info: &NowPlayingInfo) -> PlayerState {
    PlayerState {
        status: get_status(info),
        texts: get_current_track(info),
        timeline: get_timeline_info(info),
    }
}

async fn push_state(driver: Arc<dyn FsctDriver>, player_id: ManagedPlayerId, info: Option<NowPlayingInfo>) {
    if let Some(info) = info {
        let state = build_state(&info);
        let _ = driver.update_player_state(player_id, state).await;
    }
}

fn get_macos_version() -> Option<(u32, u32)> {
    let output = Command::new("sw_vers").arg("-productVersion").output().ok()?;

    let version_str = String::from_utf8(output.stdout).ok()?;
    let version_parts: Vec<&str> = version_str.trim().split('.').collect();

    if version_parts.len() >= 2 {
        let major = version_parts[0].parse::<u32>().ok()?;
        let minor = version_parts[1].parse::<u32>().ok()?;
        Some((major, minor))
    } else {
        None
    }
}

pub async fn start_macos_now_playing_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<MacOSWatcherHandle> {
    // Register a single native macOS player (for the OS global now playing)
    let player_id = driver
        .register_player("native-macos-nowplaying".to_string())
        .await
        .map_err(|e| anyhow!(e))?;

    let driver_closure = driver.clone();
    let pid_closure = player_id;
    let current_tokio_runtime = tokio::runtime::Handle::current();

    // Choose implementation based on macOS version
    if let Some((major, minor)) = get_macos_version() {
        if major > 15 || (major == 15 && minor >= 4) {
            let now_playing = NowPlayingJXA::new(Duration::from_millis(500));

            now_playing.subscribe(move |guard| {
                let d = driver_closure.clone();
                let opt = guard.as_ref().cloned();
                current_tokio_runtime.spawn(async move {
                    push_state(d, pid_closure, opt).await;
                });
            });
            // push initial state
            let initial_guard = now_playing.get_info();
            let initial = initial_guard.as_ref().cloned();
            tokio::spawn(push_state(driver.clone(), player_id, initial));

            drop(initial_guard);
            return Ok(MacOSWatcherHandle { jxa: Some(now_playing), native: None, _player_id: player_id });
        }
    }

    // Fallback to native implementation
    let now_playing = NowPlaying::new();

    now_playing.subscribe(move |guard| {
        let d = driver_closure.clone();
        let opt = guard.as_ref().cloned();
        current_tokio_runtime.spawn(async move {
            push_state(d, pid_closure, opt).await;
        });
    });
    // push initial state
    let initial_guard = now_playing.get_info();
    let initial = initial_guard.as_ref().cloned();
    tokio::spawn(push_state(driver.clone(), player_id, initial));

    drop(initial_guard);
    Ok(MacOSWatcherHandle { jxa: None, native: Some(Mutex::new(NowPlayingWrapper { now_playing })), _player_id: player_id })
}
