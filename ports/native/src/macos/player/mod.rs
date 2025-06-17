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

use async_trait::async_trait;
use fsct_core::Player;
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::player::{PlayerError, PlayerInterface, PlayerState, TrackMetadata};
use std::time::{Duration, SystemTime};
use tokio::task::spawn_blocking;

pub struct MacOSPlaybackManager {}

impl MacOSPlaybackManager {
    pub fn new() -> Result<Self, PlayerError> {
        Ok(Self {})
    }
}

fn get_text_from_now_playing_info(now_playing_info: &serde_json::Value, key: &str) -> Option<String> {
    now_playing_info["info"][key].as_str().map(|s| s.to_string())
}
fn get_current_track(now_playing_info: &serde_json::Value) -> TrackMetadata {
    let mut texts = TrackMetadata::default();
    texts.title = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoTitle");
    texts.artist = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoArtist");
    texts.album = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoAlbum");
    texts.genre = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoGenre");

    texts
}

fn get_timeline_info(now_playing_info: &serde_json::Value) -> Option<TimelineInfo> {
    let duration = now_playing_info["info"]["kMRMediaRemoteNowPlayingInfoDuration"].as_f64()?;

    let position = now_playing_info["info"]["kMRMediaRemoteNowPlayingInfoElapsedTime"]
        .as_f64()
        .unwrap_or(0.0);

    let update_time = now_playing_info["info"]["kMRMediaRemoteNowPlayingInfoTimestamp"]
        .as_u64()
        .and_then(|t| Some(SystemTime::UNIX_EPOCH + Duration::from_millis(t)))
        .unwrap_or(SystemTime::now());

    let rate = now_playing_info["info"]["kMRMediaRemoteNowPlayingInfoPlaybackRate"]
        .as_f64()
        .and_then(|v| Some(v as f32))
        .unwrap_or(0.0);

    Some(TimelineInfo {
        position: Duration::from_secs_f64(position),
        update_time,
        duration: Duration::from_secs_f64(duration),
        rate: rate as f64,
    })
}

fn get_status(now_playing_info: &serde_json::Value) -> FsctStatus {
    let current_playback_rate = now_playing_info["info"]["kMRMediaRemoteNowPlayingInfoPlaybackRate"].as_f64();
    match current_playback_rate {
        Some(0.0) => FsctStatus::Paused,
        Some(_) => FsctStatus::Playing,
        None => FsctStatus::Stopped,
    }
}

#[async_trait]
impl PlayerInterface for MacOSPlaybackManager {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        let now_playing_info = spawn_blocking(move || media_remote::get_raw_info())
            .await
            .map_err(|e| PlayerError::Other(e.into()))?;

        if let Some(now_playing_info) = now_playing_info {
            let status = get_status(&now_playing_info);
            let texts = get_current_track(&now_playing_info);
            let timeline = get_timeline_info(&now_playing_info);
            Ok(PlayerState {
                status,
                timeline,
                texts,
            })
        } else {
            Ok(PlayerState::default())
        }
    }
}

pub async fn initialize_native_platform_player() -> anyhow::Result<Player> {
    Ok(Player::new(MacOSPlaybackManager::new()?))
}
