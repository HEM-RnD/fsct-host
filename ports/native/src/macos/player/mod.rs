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
use fsct_core::service::{ServiceHandle, spawn_service};
use media_remote::{NowPlaying, NowPlayingInfo, NowPlayingJXA, Subscription};
use std::process::Command;
use std::sync::Mutex;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use anyhow::anyhow;
use tokio::sync::mpsc;

#[allow(dead_code)]
struct NowPlayingWrapper {
    now_playing: NowPlaying,
}

unsafe impl Send for NowPlayingWrapper {}


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

async fn push_state(driver: Arc<dyn FsctDriver>, player_id: ManagedPlayerId, previous_state: &mut PlayerState, info: Option<NowPlayingInfo>) {
    if let Some(info) = info {
        let state = build_state(&info);
        if *previous_state != state {
            *previous_state = state.clone();
            let _ = driver.update_player_state(player_id, state).await;
        }
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

#[allow(dead_code)]
enum NowPlayingImpl {
    JXA(NowPlayingJXA),
    Native(NowPlayingWrapper),
}

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    // Register a single native macOS player (for the OS global now playing)
    let player_id = driver
        .register_player("native-macos-nowplaying".to_string())
        .await
        .map_err(|e| anyhow!(e))?;

    // Spawn a single service task that consumes the queue and updates state
    let handle = spawn_service(move |mut stop| async move {
        // Channel to move updates from callback context to our service task
        let (tx, mut rx) = mpsc::unbounded_channel::<Option<NowPlayingInfo>>();

        // Choose implementation based on macOS version and set up subscriptions
        let _now_playing: NowPlayingImpl = if let Some((major, minor)) = get_macos_version() && (major > 15 || (major == 15 && minor >= 4)) {
                let now_playing = NowPlayingJXA::new(Duration::from_millis(500));
                let tx_clone = tx.clone();
                now_playing.subscribe(move |guard| {
                    let _ = tx_clone.send(guard.as_ref().cloned());
                });
                // push initial state via the same queue
                let initial = now_playing.get_info().as_ref().cloned();
                let _ = tx.send(initial);

                NowPlayingImpl::JXA(now_playing)
        } else {
            // Fallback to native implementation
            let now_playing = NowPlaying::new();
            let tx_clone = tx.clone();
            now_playing.subscribe(move |guard| {
                let _ = tx_clone.send(guard.as_ref().cloned());
            });
            // push initial state via the same queue
            let initial = now_playing.get_info().as_ref().cloned();
            let _ = tx.send(initial);

            NowPlayingImpl::Native(NowPlayingWrapper { now_playing })
        };

        let mut previous_state = PlayerState::default();
        loop {
            tokio::select! {
                _ = stop.signaled() => {
                    break;
                }
                maybe = rx.recv() => {
                    match maybe {
                        Some(opt) => {
                            push_state(driver.clone(), player_id, &mut previous_state, opt).await;
                        }
                        None => {
                            // Sender dropped; exit loop
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(handle)
}
