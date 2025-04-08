use async_trait::async_trait;
use std::any::Any;
use std::ops::Deref;
use std::sync::Arc;
use std::time::SystemTime;
use tokio;
use crate::definitions::TimelineInfo;
// upewnij się, że używasz asynchronicznego runtime (np. tokio)

use crate::platform::macos::media_remote::MediaRemoteFramework;
use crate::platform::{
    PlatformBehavior, PlaybackControlProvider, PlaybackInfoProvider,
    PlaybackInterface,
};
use crate::player::{PlaybackError, Player, PlayerInterface, Track};

mod media_remote; // importujemy nasz moduł FFI

pub struct MacOSPlatform;

impl MacOSPlatform {
    pub fn new() -> Self {
        MacOSPlatform
    }
}

pub struct MacOSPlaybackManager {
    media_remote: Arc<MediaRemoteFramework>,
}

#[async_trait]
impl PlayerInterface for MacOSPlaybackManager {
    async fn get_current_track(&self) -> Result<Track, PlaybackError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlaybackError::UnknownError(e))?;

        let title_value = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoTitle")
            .ok_or_else(|| PlaybackError::UnknownError("Nie znaleziono tytułu utworu".into()))?
            .downcast_ref::<String>()
            .ok_or_else(|| PlaybackError::UnknownError("Nie znaleziono tytułu utworu".into()))?
            .clone();

        let artist_value = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoArtist")
            .ok_or_else(|| PlaybackError::UnknownError("Nie znaleziono wykonawcy".into()))?
            .downcast_ref::<String>()
            .ok_or_else(|| PlaybackError::UnknownError("Nie znaleziono tytułu utworu".into()))?
            .clone();

        Ok(Track {
            title: title_value,
            artist: artist_value,
        })
    }

    async fn get_timeline_info(&self) -> Result<Option<TimelineInfo>, PlaybackError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlaybackError::UnknownError(e))?;

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

    async fn is_playing(&self) -> Result<bool, PlaybackError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlaybackError::UnknownError(e))?;

        let current_playback_rate = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoPlaybackRate")
            .and_then(|v| v.downcast_ref::<f32>())
            .cloned()
            .unwrap_or(0.0);

        let is_playing = current_playback_rate > 0.0;
        Ok(is_playing)
    }

    async fn play(&self) -> Result<(), PlaybackError> {
        // Tutaj należy umieścić wywołanie MediaRemote dla rozpoczęcia odtwarzania.
        Ok(())
    }

    async fn pause(&self) -> Result<(), PlaybackError> {
        Ok(())
    }

    async fn stop(&self) -> Result<(), PlaybackError> {
        Ok(())
    }

    async fn next_track(&self) -> Result<(), PlaybackError> {
        Ok(())
    }

    async fn previous_track(&self) -> Result<(), PlaybackError> {
        Ok(())
    }
}

#[async_trait]
impl PlatformBehavior for MacOSPlatform {
    fn get_platform_name(&self) -> &'static str {
        "macOS"
    }

    async fn initialize(&self) -> Result<Player, String> {
        let media_remote = Arc::new(MediaRemoteFramework::load()?);
        let playback_manager: Arc<dyn PlaybackInfoProvider> = Arc::new(MacOSPlaybackManager {
            media_remote: media_remote.clone(),
        });

        Ok(Player::new(playback_manager))
    }

    async fn cleanup(&self) -> Result<(), String> {
        Ok(())
    }
}
