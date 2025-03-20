use async_trait::async_trait;
use std::any::Any;
use std::ops::Deref;
use std::sync::Arc;
use std::time::SystemTime;
use tokio; // upewnij się, że używasz asynchronicznego runtime (np. tokio)

use crate::platform::macos::media_remote::MediaRemoteFramework;
use crate::platform::{
    PlatformBehavior, PlatformContext, PlaybackControlProvider, PlaybackError,
    PlaybackInfoProvider, TimelineInfo, Track,
};

mod media_remote; // importujemy nasz moduł FFI

pub struct MacOSPlatform;

impl MacOSPlatform {
    pub fn new() -> Self {
        MacOSPlatform
    }
}

/// Implementacja PlaybackInfoProvider dla macOS wykorzystująca MediaRemote.framework.
pub struct MacOSPlaybackInfoProvider {
    media_remote: Arc<MediaRemoteFramework>,
}

#[async_trait]
impl PlaybackInfoProvider for MacOSPlaybackInfoProvider {
    async fn get_current_track(&self) -> Result<Track, PlaybackError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlaybackError::UnknownError(e))?;

        // Funkcja find zwraca Option<&CFType>; zakładamy, że wartości są CFString.
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

    async fn get_timeline_info(&self) -> Result<TimelineInfo, PlaybackError> {
        let now_playing_info = self
            .media_remote
            .get_now_playing_info()
            .await
            .map_err(|e| PlaybackError::UnknownError(e))?;

        // Próba pobrania czasu trwania utworu – zakładamy, że zwracana wartość to napis reprezentujący liczbę sekund.
        let duration = now_playing_info
            .get("kMRMediaRemoteNowPlayingInfoDuration")
            .and_then(|v| v.downcast_ref::<f64>())
            .cloned();

        // Spróbuj pobrać informację o bieżącej pozycji – zakładamy klucz "kMRMediaRemoteNowPlayingInfoElapsedTime"
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
        let is_playing = current_playback_rate > 0.0;

        Ok(TimelineInfo {
            position,
            update_time,
            duration,
            is_playing,
            playback_rate: current_playback_rate,
        })
    }

    async fn is_playing(&self) -> Result<bool, PlaybackError> {
        let timeline = self.get_timeline_info().await?;
        Ok(timeline.is_playing)
    }

    async fn get_volume(&self) -> Result<u8, PlaybackError> {
        // Jeśli MediaRemote nie udostępnia poziomu głośności – zwracamy przykładową wartość.
        Ok(50)
    }
}

/// Implementacja PlaybackControlProvider dla macOS (pozostaje przykładowa).
pub struct MacOSPlaybackControlProvider;

#[async_trait]
impl PlaybackControlProvider for MacOSPlaybackControlProvider {
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

    async fn set_volume(&self, _volume: u8) -> Result<(), PlaybackError> {
        Ok(())
    }
}

#[async_trait]
impl PlatformBehavior for MacOSPlatform {
    fn get_platform_name(&self) -> &'static str {
        "macOS"
    }

    async fn initialize(&self) -> Result<PlatformContext, String> {
        let media_remote = Arc::new(MediaRemoteFramework::load()?);
        let info_provider: Arc<dyn PlaybackInfoProvider> = Arc::new(MacOSPlaybackInfoProvider {
            media_remote: media_remote.clone(),
        });
        let control_provider: Arc<dyn PlaybackControlProvider> =
            Arc::new(MacOSPlaybackControlProvider);
        Ok(PlatformContext {
            info: info_provider,
            control: control_provider,
        })
    }

    async fn cleanup(&self) -> Result<(), String> {
        Ok(())
    }
}
