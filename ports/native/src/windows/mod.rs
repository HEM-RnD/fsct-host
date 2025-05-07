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

use std::time::Duration;
use async_trait::async_trait;
use windows::{
    core::Error as WindowsError,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
    },
};
use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionMediaProperties, GlobalSystemMediaTransportControlsSessionPlaybackInfo, GlobalSystemMediaTransportControlsSessionTimelineProperties};
use fsct_core::definitions::{TimelineInfo};
use fsct_core::player::{PlayerError, PlayerInterface, PlayerState, TrackMetadata};
use fsct_core::definitions::FsctStatus;

trait IntoPlayerResult<T> {
    fn into_player_error(self) -> Result<T, PlayerError>;
}

impl<T> IntoPlayerResult<T> for Result<T, WindowsError> {
    fn into_player_error(self) -> Result<T, PlayerError> {
        self.map_err(|e| PlayerError::Other(e.into()))
    }
}

pub struct WindowsPlatformGlobalSessionManager {
    session_manager: GlobalSystemMediaTransportControlsSessionManager,
}

const UNIX_EPOCH_OFFSET: i64 = 116444736000000000;

impl WindowsPlatformGlobalSessionManager {
    pub async fn new() -> Result<Self, PlayerError> {
        let session_manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .into_player_error()?
            .await
            .into_player_error()?;

        Ok(Self { session_manager })
    }

    async fn get_session(&self) -> Result<GlobalSystemMediaTransportControlsSession, PlayerError> {
        let session = self.session_manager
                          .GetCurrentSession().into_player_error()?;
        Ok(session)
    }

    async fn get_media_properties(&self) -> Result<GlobalSystemMediaTransportControlsSessionMediaProperties, PlayerError> {
        Ok(self.get_session().await?.TryGetMediaPropertiesAsync().into_player_error()?.await.into_player_error()?)
    }
}

async fn get_timeline_info(playback_info: &GlobalSystemMediaTransportControlsSessionPlaybackInfo,
                           timeline_properties: &GlobalSystemMediaTransportControlsSessionTimelineProperties, ) ->
Result<Option<TimelineInfo>, PlayerError> {
    let position = timeline_properties.Position().into_player_error()?;
    let last_update_time = timeline_properties.LastUpdatedTime().into_player_error()?;
    let end_time = timeline_properties.EndTime().into_player_error()?.Duration as f64 / 10_000_000.0;

    let update_time = if last_update_time.UniversalTime < UNIX_EPOCH_OFFSET {
        std::time::SystemTime::now()
    } else {
        let last_update_unix_nanos = (last_update_time.UniversalTime - UNIX_EPOCH_OFFSET) * 100;
        std::time::UNIX_EPOCH + std::time::Duration::from_nanos(last_update_unix_nanos as u64)
    };

    let position_sec = position.Duration as f64 / 10_000_000.0;

    let rate = get_rate(playback_info);

    Ok(Some(TimelineInfo {
        position: Duration::from_secs_f64(position_sec),
        update_time,
        duration: Duration::from_secs_f64(end_time),
        rate,
    }))
}

fn get_status(playback_info: &GlobalSystemMediaTransportControlsSessionPlaybackInfo) -> FsctStatus {
    match playback_info.PlaybackStatus().unwrap_or(windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed) {
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => FsctStatus::Playing,
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => FsctStatus::Paused,
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped => FsctStatus::Stopped,
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing => FsctStatus::Seeking,
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed => FsctStatus::Unknown,
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Opened => FsctStatus::Stopped,
        _ => FsctStatus::Unknown,
    }
}

fn windows_string_convert(winstr: windows_core::Result<windows_core::HSTRING>) -> Option<String> {
    winstr.map(|v| v.to_string()).ok()
}
fn get_texts(media_properties: &GlobalSystemMediaTransportControlsSessionMediaProperties) -> TrackMetadata {
    let mut texts = TrackMetadata::default();

    texts.title = windows_string_convert(media_properties.Title());
    texts.artist = windows_string_convert(media_properties.Artist());
    texts.album = windows_string_convert(media_properties.AlbumTitle());

    texts
}

#[async_trait]
impl PlayerInterface for WindowsPlatformGlobalSessionManager {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        let session = self.get_session().await?;
        let playback_info = session.GetPlaybackInfo().into_player_error()?;
        let timeline_properties = session.GetTimelineProperties().into_player_error()?;
        let media_properties = self.get_media_properties().await?;
        let timeline = get_timeline_info(&playback_info, &timeline_properties).await?;
        let status = get_status(&playback_info);
        let texts = get_texts(&media_properties);
        Ok(PlayerState {
            status,
            timeline,
            texts,
        })
    }

    async fn play(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TryPlayAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn pause(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TryPauseAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn stop(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TryStopAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn next_track(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TrySkipNextAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn previous_track(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TrySkipPreviousAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }
}

fn get_rate(playback_info: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackInfo) -> f64 {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
    if playback_info.PlaybackStatus().unwrap_or(PlaybackStatus::Closed) != PlaybackStatus::Playing {
        return 0.0;
    }
    playback_info.PlaybackRate().map(|rate| rate.Value().unwrap_or(1.0)).unwrap_or(1.0)
}


