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
use fsct_core::definitions::FsctStatus;
use fsct_core::definitions::TimelineInfo;
use fsct_core::player::{PlayerError, PlayerInterface, PlayerState, TrackMetadata};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use fsct_core::Player;

mod media_remote;
pub mod service;

use media_remote::MediaRemoteFramework;
use crate::windows::WindowsPlatformGlobalSessionManager;

pub struct MacOSPlaybackManager {
    media_remote: Arc<MediaRemoteFramework>,
}

impl MacOSPlaybackManager {
    pub fn new() -> Result<Self, PlayerError> {
        let media_remote = Arc::new(MediaRemoteFramework::load().map_err(|e| PlayerError::Other(e))?);
        Ok(MacOSPlaybackManager { media_remote })
    }
}

fn get_text_from_now_playing_info(
    now_playing_info: &HashMap<String, Box<dyn Any + Send>>,
    key: &str,
) -> Option<String> {
    now_playing_info
        .get(key)
        .and_then(|v| v.downcast_ref::<String>())
        .cloned()
}
fn get_current_track(now_playing_info: &HashMap<String, Box<dyn Any + Send>>) -> TrackMetadata {
    let mut texts = TrackMetadata::default();
    texts.title = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoTitle");
    texts.artist = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoArtist");
    texts.album = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoAlbum");
    texts.genre = get_text_from_now_playing_info(now_playing_info, "kMRMediaRemoteNowPlayingInfoGenre");

    texts
}

fn get_timeline_info(now_playing_info: &HashMap<String, Box<dyn Any + Send>>) -> Option<TimelineInfo> {
    let duration = now_playing_info
        .get("kMRMediaRemoteNowPlayingInfoDuration")
        .and_then(|v| v.downcast_ref::<f64>())
        .cloned()?;

    let position = now_playing_info
        .get("kMRMediaRemoteNowPlayingInfoElapsedTime")
        .and_then(|v| v.downcast_ref::<f64>())
        .cloned()
        .unwrap_or(0.0);

    let update_time = now_playing_info
        .get("kMRMediaRemoteNowPlayingInfoTimestamp")
        .and_then(|v| v.downcast_ref::<std::time::SystemTime>())
        .cloned()
        .unwrap_or(SystemTime::now());

    let rate = now_playing_info
        .get("kMRMediaRemoteNowPlayingInfoPlaybackRate")
        .and_then(|v| v.downcast_ref::<f32>())
        .cloned()
        .unwrap_or(0.0);

    Some(TimelineInfo {
        position: Duration::from_secs_f64(position),
        update_time,
        duration: Duration::from_secs_f64(duration),
        rate: rate as f64,
    })
}

fn get_status(now_playing_info: &HashMap<String, Box<dyn Any + Send>>) -> FsctStatus {
    let current_playback_rate = now_playing_info
        .get("kMRMediaRemoteNowPlayingInfoPlaybackRate")
        .and_then(|v| v.downcast_ref::<f32>())
        .cloned();
    match current_playback_rate {
        Some(0.0) => FsctStatus::Paused,
        Some(_) => FsctStatus::Playing,
        None => FsctStatus::Stopped,
    }
}

#[async_trait]
impl PlayerInterface for MacOSPlaybackManager {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await?;

        let status = get_status(&now_playing_info);
        let texts = get_current_track(&now_playing_info);
        let timeline = get_timeline_info(&now_playing_info);
        Ok(PlayerState {
            status,
            timeline,
            texts,
        })
    }
}

pub mod player {
    use fsct_core::Player;
    pub async fn initialize_native_platform_player() -> anyhow::Result<Player> {
        Ok(Player::new(MacOSPlaybackManager::new()?))
    }

}