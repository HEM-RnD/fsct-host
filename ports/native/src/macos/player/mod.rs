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
use fsct_core::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use fsct_core::player::{
    PlayerError, PlayerEvent, PlayerEventsReceiver, PlayerEventsSender, PlayerInterface, PlayerState, TrackMetadata,
    create_player_events_channel,
};
use media_remote::{NowPlaying, NowPlayingInfo, NowPlayingJXA, Subscription};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub struct MacOSPlaybackManagerJXA {
    now_playing: NowPlayingJXA,
    player_sender: PlayerEventsSender,
}

struct NowPlayingWrapper {
    now_playing: NowPlaying,
}

unsafe impl Send for NowPlayingWrapper {}

pub struct MacOSPlaybackManagerNative {
    now_playing: Mutex<NowPlayingWrapper>,
    player_sender: PlayerEventsSender,
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

fn send_changes(info: &Option<NowPlayingInfo>, tx: &PlayerEventsSender) {
    if let Some(info) = info.as_ref() {
        tx.send(PlayerEvent::TextChanged((
            FsctTextMetadata::CurrentTitle,
            info.title.clone(),
        )))
        .unwrap_or_default();
        tx.send(PlayerEvent::TextChanged((
            FsctTextMetadata::CurrentAuthor,
            info.artist.clone(),
        )))
        .unwrap_or_default();
        tx.send(PlayerEvent::TextChanged((
            FsctTextMetadata::CurrentAlbum,
            info.album.clone(),
        )))
        .unwrap_or_default();
        tx.send(PlayerEvent::StatusChanged(get_status(info)))
            .unwrap_or_default();
        tx.send(PlayerEvent::TimelineChanged(get_timeline_info(info)))
            .unwrap_or_default();
    }
}

impl MacOSPlaybackManagerJXA {
    pub fn new() -> Result<Self, PlayerError> {
        let (player_sender, _rx) = create_player_events_channel();
        let tx = player_sender.clone();
        let now_playing = NowPlayingJXA::new(Duration::from_millis(500));
        now_playing.subscribe(move |info| {
            send_changes(&info, &tx);
        });
        Ok(Self {
            now_playing,
            player_sender,
        })
    }
}

impl MacOSPlaybackManagerNative {
    pub fn new() -> Result<Self, PlayerError> {
        let (player_sender, _rx) = create_player_events_channel();
        let tx = player_sender.clone();
        let now_playing = NowPlaying::new();
        now_playing.subscribe(move |info| {
            send_changes(&info, &tx);
        });
        Ok(Self {
            now_playing: Mutex::new(NowPlayingWrapper { now_playing }),
            player_sender,
        })
    }
}

fn get_current_state(info: &Option<NowPlayingInfo>) -> Result<PlayerState, PlayerError> {
    if let Some(info) = info {
        Ok(PlayerState {
            status: get_status(info),
            texts: get_current_track(info),
            timeline: get_timeline_info(info),
        })
    } else {
        Err(PlayerError::PermissionDenied)
    }
}

#[async_trait]
impl PlayerInterface for MacOSPlaybackManagerJXA {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        let info = self.now_playing.get_info();
        get_current_state(&info)
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        let rx = self.player_sender.subscribe();
        let info = self.now_playing.get_info();
        send_changes(&info, &self.player_sender);
        Ok(rx)
    }
}

#[async_trait]
impl PlayerInterface for MacOSPlaybackManagerNative {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        let now_playing = self.now_playing.lock().unwrap();
        let info = now_playing.now_playing.get_info();
        get_current_state(&info)
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        let rx = self.player_sender.subscribe();
        let now_playing = self.now_playing.lock().unwrap();
        let info = now_playing.now_playing.get_info();
        send_changes(&info, &self.player_sender);
        Ok(rx)
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

pub async fn initialize_native_platform_player() -> anyhow::Result<Player> {
    // Check macOS version
    if let Some((major, minor)) = get_macos_version() {
        // For macOS 15.4 and newer, use JXA
        if major > 15 || (major == 15 && minor >= 4) {
            return Ok(Player::new(MacOSPlaybackManagerJXA::new()?));
        }
    }

    // For older versions or if version detection fails, use Native
    Ok(Player::new(MacOSPlaybackManagerNative::new()?))
}
