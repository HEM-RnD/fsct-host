use std::sync::Arc;
use async_trait::async_trait;
use windows::{
    core::Error as WindowsError,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
    },
};
use crate::definitions::TimelineInfo;
use crate::player::{PlayerError, Player, PlayerInterface, Track};
use super::PlatformBehavior;

pub struct WindowsPlatform;

impl WindowsPlatform {
    pub fn new() -> Self {
        WindowsPlatform
    }
}

pub struct WindowsPlatformGlobalSessionManager {
    session_manager: GlobalSystemMediaTransportControlsSessionManager,
}

const UNIX_EPOCH_OFFSET: i64 = 116444736000000000;

impl WindowsPlatformGlobalSessionManager {
    async fn new() -> Result<Self, PlayerError> {
        let session_manager_result = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map_err(|e| PlayerError::UnknownError(e.to_string()))?
            .await;


        let session_manager = session_manager_result.map_err(|e| PlayerError::UnknownError(e.to_string()))?;

        Ok(Self { session_manager })
    }

    async fn get_session(&self) -> Result<GlobalSystemMediaTransportControlsSession, PlayerError> {
        let session = self.session_manager
                          .GetCurrentSession()
                          .map_err(|e| PlayerError::UnknownError(e.to_string()))?;
        Ok(session)
    }

    async fn get_media_properties(&self) -> Result<windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties, PlayerError> {
        Ok(self.get_session().await?.TryGetMediaPropertiesAsync()?.await?)
    }
}

#[async_trait]
impl PlayerInterface for WindowsPlatformGlobalSessionManager {
    async fn get_current_track(&self) -> Result<Track, PlayerError> {
        let props = self.get_media_properties()
                        .await
                        .map_err(|e| PlayerError::UnknownError(e.to_string()))?;

        Ok(Track {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
        })
    }

    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlayerError> {
        let session = self.get_session().await?;
        let timeline = session.GetTimelineProperties()?;
        let position = timeline.Position()?;
        let last_update_time = timeline.LastUpdatedTime()?;
        let end_time = timeline.EndTime()?.Duration as f64 / 10_000_000.0;

        let update_time = if last_update_time.UniversalTime < UNIX_EPOCH_OFFSET {
            std::time::SystemTime::now()
        } else {
            let last_update_unix_nanos = (last_update_time.UniversalTime - UNIX_EPOCH_OFFSET) * 100;
            std::time::UNIX_EPOCH + std::time::Duration::from_nanos(last_update_unix_nanos as u64)
        };

        let position_sec = position.Duration as f64 / 10_000_000.0;

        let playback_info = session.GetPlaybackInfo()?;
        let rate = get_rate(&playback_info);

        Ok(Some(TimelineInfo {
            position: position_sec,
            update_time,
            duration: end_time,
            rate,
        }))
    }

    async fn is_playing(&self) -> Result<bool, PlayerError> {
        let session = self.get_session().await?;
        let playback_info = session.GetPlaybackInfo()?;
        let is_playing = playback_info.PlaybackStatus()? == windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        Ok(is_playing)
    }

    async fn play(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TryPlayAsync()?.await?;
        Ok(())
    }

    async fn pause(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TryPauseAsync()?.await?;
        Ok(())
    }

    async fn stop(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TryStopAsync()?.await?;
        Ok(())
    }

    async fn next_track(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TrySkipNextAsync()?.await?;
        Ok(())
    }

    async fn previous_track(&self) -> Result<(), PlayerError> {
        self.get_session().await?.TrySkipPreviousAsync()?.await?;
        Ok(())
    }
}

fn get_rate(playback_info: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackInfo) -> f32 {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
    if playback_info.PlaybackStatus().unwrap_or(PlaybackStatus::Closed) != PlaybackStatus::Playing {
        return 0.0;
    }
    playback_info.PlaybackRate().map(|rate| rate.Value().unwrap_or(1.0)).unwrap_or(1.0) as f32
}

#[async_trait]
impl PlatformBehavior for WindowsPlatform {
    fn get_platform_name(&self) -> &'static str {
        "Windows"
    }

    async fn initialize(&self) -> Result<Player, String> {
        let global_session_manager = Arc::new(WindowsPlatformGlobalSessionManager::new()
            .await
            .map_err(|e| e.to_string())?);

        Ok(Player::new(global_session_manager))
    }

    async fn cleanup(&self) -> Result<(), String> {
        Ok(())
    }
}

impl From<WindowsError> for PlayerError {
    fn from(err: WindowsError) -> Self {
        PlayerError::UnknownError(err.to_string())
    }
}

