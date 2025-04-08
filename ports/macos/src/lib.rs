use async_trait::async_trait;
use std::any::Any;
use std::ops::Deref;
use std::sync::Arc;
use std::time::SystemTime;
use tokio;
use fsct_core::definitions::TimelineInfo;
use fsct_core::player::{PlayerError, PlayerInterface, Track};

mod media_remote;

use crate::media_remote::MediaRemoteFramework;


pub struct MacOSPlatform;

impl MacOSPlatform {
    pub fn new() -> Self {
        MacOSPlatform
    }
}

pub struct MacOSPlaybackManager {
    media_remote: Arc<MediaRemoteFramework>,
}

impl MacOSPlaybackManager {
    pub fn new() -> Result<Self, PlayerError> {
        let media_remote = Arc::new(MediaRemoteFramework::load().map_err(|e| PlayerError::UnknownError(e))?);
        Ok(MacOSPlaybackManager { media_remote })
    }
}

#[async_trait]
impl PlayerInterface for MacOSPlaybackManager {
    async fn get_current_track(&self) -> Result<Track, PlayerError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlayerError::UnknownError(e))?;

        let title_value = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoTitle")
            .ok_or_else(|| PlayerError::UnknownError("No track title found".into()))?
            .downcast_ref::<String>()
            .ok_or_else(|| PlayerError::UnknownError("No track title found".into()))?
            .clone();

        let artist_value = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoArtist")
            .ok_or_else(|| PlayerError::UnknownError("No track artist found".into()))?
            .downcast_ref::<String>()
            .ok_or_else(|| PlayerError::UnknownError("No track artist found".into()))?
            .clone();

        Ok(Track {
            title: title_value,
            artist: artist_value,
        })
    }

    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlayerError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlayerError::UnknownError(e))?;

        let duration = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoDuration")
            .and_then(|v| v.downcast_ref::<f64>())
            .cloned();

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

        let current_playback_rate = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoPlaybackRate")
            .and_then(|v| v.downcast_ref::<f32>())
            .cloned()
            .unwrap_or(0.0);

        if duration.is_none() {
            return Ok(None);
        }

        Ok(Some(TimelineInfo {
            position,
            update_time,
            duration: duration.unwrap(),
            rate: current_playback_rate,
        }))
    }

    async fn is_playing(&self) -> Result<bool, PlayerError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlayerError::UnknownError(e))?;

        let current_playback_rate = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoPlaybackRate")
            .and_then(|v| v.downcast_ref::<f32>())
            .cloned()
            .unwrap_or(0.0);

        let is_playing = current_playback_rate > 0.0;
        Ok(is_playing)
    }
}
