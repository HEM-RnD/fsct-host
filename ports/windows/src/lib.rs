use async_trait::async_trait;
use windows::{
    core::Error as WindowsError,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
    },
};
use fsct_core::definitions::TimelineInfo;
use fsct_core::player::{PlayerError, PlayerInterface, Track};


trait IntoPlayerResult<T> {
    fn into_player_error(self) -> Result<T, PlayerError>;
}

impl<T> IntoPlayerResult<T> for Result<T, WindowsError> {
    fn into_player_error(self) -> Result<T, PlayerError> {
        self.map_err(|e| PlayerError::UnknownError(e.to_string()))
    }
}

pub struct WindowsPlatformGlobalSessionManager {
    session_manager: GlobalSystemMediaTransportControlsSessionManager,
}

const UNIX_EPOCH_OFFSET: i64 = 116444736000000000;

impl WindowsPlatformGlobalSessionManager {
    pub async fn new() -> Result<Self, PlayerError> {
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
        Ok(self.get_session().await?.TryGetMediaPropertiesAsync().into_player_error()?.await.into_player_error()?)
    }
}

#[async_trait]
impl PlayerInterface for WindowsPlatformGlobalSessionManager {
    async fn get_current_track(&self) -> Result<Track, PlayerError> {
        let props = self.get_media_properties()
                        .await
                        .map_err(|e| PlayerError::UnknownError(e.to_string()))?;

        Ok(Track {
            title: props.Title().into_player_error()?.to_string(),
            artist: props.Artist().into_player_error()?.to_string(),
        })
    }

    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlayerError> {
        let session = self.get_session().await?;
        let timeline = session.GetTimelineProperties().into_player_error()?;
        let position = timeline.Position().into_player_error()?;
        let last_update_time = timeline.LastUpdatedTime().into_player_error()?;
        let end_time = timeline.EndTime().into_player_error()?.Duration as f64 / 10_000_000.0;

        let update_time = if last_update_time.UniversalTime < UNIX_EPOCH_OFFSET {
            std::time::SystemTime::now()
        } else {
            let last_update_unix_nanos = (last_update_time.UniversalTime - UNIX_EPOCH_OFFSET) * 100;
            std::time::UNIX_EPOCH + std::time::Duration::from_nanos(last_update_unix_nanos as u64)
        };

        let position_sec = position.Duration as f64 / 10_000_000.0;

        let playback_info = session.GetPlaybackInfo().into_player_error()?;
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
        let playback_info = session.GetPlaybackInfo().into_player_error()?;
        let is_playing = playback_info.PlaybackStatus().into_player_error()? == windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        Ok(is_playing)
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

fn get_rate(playback_info: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackInfo) -> f32 {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
    if playback_info.PlaybackStatus().unwrap_or(PlaybackStatus::Closed) != PlaybackStatus::Playing {
        return 0.0;
    }
    playback_info.PlaybackRate().map(|rate| rate.Value().unwrap_or(1.0)).unwrap_or(1.0) as f32
}

// struct WindowsPlayerError(PlayerError);
// 
// impl From<WindowsError> for WindowsPlayerError {
//     fn from(err: WindowsError) -> Self {
//         Self(PlayerError::UnknownError(err.to_string()))
//     }
// }
// 
// impl From<WindowsPlayerError> for PlayerError {
//     fn from(err: WindowsPlayerError) -> Self {
//         err.0
//     }
// }

// impl From<WindowsError> for PlayerError {
//     fn from(err: WindowsError) -> Self {
//         PlayerError::UnknownError(err.to_string())
//     }
// }

