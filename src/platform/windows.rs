use std::sync::Arc;
use async_trait::async_trait;
use windows::{
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
    },
    core::Error as WindowsError,
};

use super::{
    PlaybackError, 
    Track, 
    PlaybackInfoProvider, 
    PlaybackControlProvider, 
    PlatformContext,
    PlatformBehavior,
    TimelineInfo,
};

pub struct WindowsPlatform;

impl WindowsPlatform {
    pub fn new() -> Self {
        WindowsPlatform
    }
}

pub struct WindowsPlaybackInfo {
    session: GlobalSystemMediaTransportControlsSession,
}

const UNIX_EPOCH_OFFSET: i64 = 116444736000000000;

impl WindowsPlaybackInfo {
    async fn new() -> Result<Self, PlaybackError> {
        let session_manager_result = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map_err(|e| PlaybackError::UnknownError(e.to_string()))?
            .await;


        let session_manager = session_manager_result.map_err(|e| PlaybackError::UnknownError(e.to_string()))?;

        let session = session_manager
            .GetCurrentSession()
            .map_err(|e| PlaybackError::UnknownError(e.to_string()))?;

        Ok(Self { session })
    }

    async fn get_media_properties(&self) -> Result<windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties, PlaybackError> {
        Ok(self.session.TryGetMediaPropertiesAsync()?.await?)
    }
}

#[async_trait]
impl PlaybackInfoProvider for WindowsPlaybackInfo {
    async fn get_current_track(&self) -> Result<Track, PlaybackError> {
        let props = self.get_media_properties()
            .await
            .map_err(|e| PlaybackError::UnknownError(e.to_string()))?;

        Ok(Track {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
        })
    }

    async fn get_timeline_info(&self) -> Result<TimelineInfo, PlaybackError> {
        let timeline = self.session.GetTimelineProperties()?;
        let position = timeline.Position()?;
        let last_update_time = timeline.LastUpdatedTime()?;
        let end_time = timeline.EndTime()?.Duration as f64 / 10_000_000.0;

        let last_update_unix_nanos = (last_update_time.UniversalTime - UNIX_EPOCH_OFFSET) * 100;
        let update_time = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(last_update_unix_nanos as u64);
        
        let position_sec = position.Duration as f64 / 10_000_000.0;
        
        let playback_info = self.session.GetPlaybackInfo()?;
        let is_playing = playback_info.PlaybackStatus()? == windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;

        Ok(TimelineInfo {
            position: position_sec,
            update_time,
            duration: Some(end_time),
            is_playing,
        })
    }

    async fn is_playing(&self) -> Result<bool, PlaybackError> {
        let timeline_info = self.get_timeline_info().await?;
        Ok(timeline_info.is_playing)
    }

    async fn get_volume(&self) -> Result<u8, PlaybackError> {
        Err(PlaybackError::FeatureNotSupported)
    }
}

pub struct WindowsPlaybackControl {
    session: GlobalSystemMediaTransportControlsSession,
}

#[async_trait]
impl PlaybackControlProvider for WindowsPlaybackControl {
    async fn play(&self) -> Result<(), PlaybackError> {
        self.session.TryPlayAsync()?.await?;
        Ok(())
    }

    async fn pause(&self) -> Result<(), PlaybackError> {
        self.session.TryPauseAsync()?.await?;
        Ok(())
    }

    async fn stop(&self) -> Result<(), PlaybackError> {
        self.session.TryStopAsync()?.await?;
        Ok(())
    }

    async fn next_track(&self) -> Result<(), PlaybackError> {
        self.session.TrySkipNextAsync()?.await?;
        Ok(())
    }

    async fn previous_track(&self) -> Result<(), PlaybackError> {
        self.session.TrySkipPreviousAsync()?.await?;
        Ok(())
    }

    async fn set_volume(&self, _volume: u8) -> Result<(), PlaybackError> {
        Err(PlaybackError::FeatureNotSupported)
    }
}

#[async_trait]
impl PlatformBehavior for WindowsPlatform {
    fn get_platform_name(&self) -> &'static str {
        "Windows"
    }

    async fn initialize(&self) -> Result<PlatformContext, String> {
        let playback_info = WindowsPlaybackInfo::new()
            .await
            .map_err(|e| e.to_string())?;

        let session: GlobalSystemMediaTransportControlsSession = playback_info.session.clone();
        
        let playback_control = WindowsPlaybackControl {
            session,
        };

        Ok(PlatformContext {
            info: Arc::new(playback_info),
            control: Arc::new(playback_control),
        })
    }

    async fn cleanup(&self) -> Result<(), String> {
        Ok(())
    }
} 

impl From<WindowsError> for PlaybackError {
    fn from(err: WindowsError) -> Self {
        PlaybackError::UnknownError(err.to_string())
    }
} 
