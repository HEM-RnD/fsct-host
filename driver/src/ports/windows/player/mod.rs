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

use crate::{JoinableTaskHandle, spawn_service};
use anyhow::Error as AnyError;
use fsct::FsctDriver;
use fsct::definitions::{FsctStatus, ManagedPlayerId, TimelineInfo};
use fsct::mono_clock::instant_from_wall;
use fsct::player_state::{PlayerState, TrackMetadata};
use log::{debug, error, warn};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};
use thiserror::Error;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSessionMediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackInfo, GlobalSystemMediaTransportControlsSessionTimelineProperties,
    MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, TimelinePropertiesChangedEventArgs,
};
use windows::{
    Media::Control::{GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager},
    core::Error as WindowsError,
};
use windows_core::HRESULT;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("Can't access player")]
    PermissionDenied,
    #[error("Player not found")]
    PlayerNotFound,
    #[error("Other error: {0}")]
    Other(#[from] AnyError),
}

/// Position/duration/update_time read straight from GSMTC timeline properties.
/// The extrapolation rate is intentionally NOT part of this — it depends on the
/// playback status, which is tracked in `CombinedState` (the single source of truth).
#[derive(Clone)]
struct TimelineParts {
    position: Duration,
    duration: Duration,
    update_time: Instant,
}

fn get_timeline_parts(
    timeline_properties: &GlobalSystemMediaTransportControlsSessionTimelineProperties,
) -> Result<TimelineParts, PlayerError> {
    let position = timeline_properties.Position().into_player_error()?;
    let last_update_time = timeline_properties.LastUpdatedTime().into_player_error()?;
    let end_time = timeline_properties.EndTime().into_player_error()?.Duration as f64 / 10_000_000.0;

    let update_time = if last_update_time.UniversalTime < UNIX_EPOCH_OFFSET {
        Instant::now()
    } else {
        let last_update_unix_nanos = (last_update_time.UniversalTime - UNIX_EPOCH_OFFSET) * 100;
        let wall = UNIX_EPOCH + Duration::from_nanos(last_update_unix_nanos as u64);
        instant_from_wall(wall)
    };

    let position_sec = position.Duration as f64 / 10_000_000.0;

    Ok(TimelineParts {
        position: Duration::from_secs_f64(position_sec),
        duration: Duration::from_secs_f64(end_time),
        update_time,
    })
}

fn get_timeline_info(
    playback_info: Option<&GlobalSystemMediaTransportControlsSessionPlaybackInfo>,
    timeline_properties: &GlobalSystemMediaTransportControlsSessionTimelineProperties,
) -> Result<Option<TimelineInfo>, PlayerError> {
    let parts = get_timeline_parts(timeline_properties)?;
    Ok(Some(TimelineInfo {
        position: parts.position,
        duration: parts.duration,
        update_time: parts.update_time,
        rate: get_rate(playback_info),
    }))
}

/// Rate gated by playback status: a non-Playing status always extrapolates at 0,
/// regardless of the rate the OS reports. Mirrors the Linux port's `effective_rate`.
fn effective_rate(status: FsctStatus, reported_rate: f64) -> f64 {
    if status == FsctStatus::Playing {
        reported_rate
    } else {
        0.0
    }
}

/// Ungated playback rate as reported by GSMTC (defaults to 1.0 when unavailable).
fn get_reported_rate(playback_info: &GlobalSystemMediaTransportControlsSessionPlaybackInfo) -> f64 {
    playback_info
        .PlaybackRate()
        .map(|rate| rate.Value().unwrap_or(1.0))
        .unwrap_or(1.0)
}

/// Single source of truth combining the two racing GSMTC events: playback status
/// (incl. reported rate) from PlaybackInfoChanged and the timeline parts from
/// TimelinePropertiesChanged. The emitted timeline's rate is always recomputed from
/// `status` here, so it can never lag the status by one transition.
#[derive(Default)]
struct CombinedState {
    status: FsctStatus,
    reported_rate: f64,
    parts: Option<TimelineParts>,
    last_timeline: Option<TimelineInfo>,
}

fn build_timeline(state: &CombinedState) -> Option<TimelineInfo> {
    let parts = state.parts.as_ref()?;
    Some(TimelineInfo {
        position: parts.position,
        duration: parts.duration,
        update_time: parts.update_time,
        rate: effective_rate(state.status, state.reported_rate),
    })
}

fn read_combined_state(session: &GlobalSystemMediaTransportControlsSession) -> CombinedState {
    let playback_info = session.GetPlaybackInfo().into_player_error().ok();
    let (status, reported_rate) = match playback_info.as_ref() {
        Some(info) => (get_status(info), get_reported_rate(info)),
        None => (FsctStatus::Unknown, 0.0),
    };
    let parts = session
        .GetTimelineProperties()
        .into_player_error()
        .ok()
        .and_then(|tp| get_timeline_parts(&tp).ok());
    let mut state = CombinedState {
        status,
        reported_rate,
        parts,
        last_timeline: None,
    };
    state.last_timeline = build_timeline(&state);
    state
}

fn get_status(playback_info: &GlobalSystemMediaTransportControlsSessionPlaybackInfo) -> FsctStatus {
    match playback_info
        .PlaybackStatus()
        .unwrap_or(windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed)
    {
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => {
            FsctStatus::Playing
        }
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => FsctStatus::Paused,
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped => {
            FsctStatus::Stopped
        }
        windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing => {
            FsctStatus::Seeking
        }
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

async fn get_texts_from_session(
    session: &GlobalSystemMediaTransportControlsSession,
) -> Result<TrackMetadata, PlayerError> {
    let media_properties = session
        .TryGetMediaPropertiesAsync()
        .into_player_error()?
        .await
        .into_player_error()?;
    Ok(get_texts(&media_properties))
}

fn get_rate(playback_info: Option<&GlobalSystemMediaTransportControlsSessionPlaybackInfo>) -> f64 {
    if let Some(playback_info) = playback_info {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
        if playback_info.PlaybackStatus().unwrap_or(PlaybackStatus::Closed) != PlaybackStatus::Playing {
            return 0.0;
        }
        playback_info
            .PlaybackRate()
            .map(|rate| rate.Value().unwrap_or(1.0))
            .unwrap_or(1.0)
    } else {
        0.0
    }
}

async fn get_playback_state(session: &GlobalSystemMediaTransportControlsSession) -> Result<PlayerState, PlayerError> {
    let playback_info = session
        .GetPlaybackInfo()
        .into_player_error()
        .inspect_err(|e| error!("[WindowsPlayer] Failed to get playback info: {:?}", e))
        .ok();
    let status = playback_info
        .as_ref()
        .map(|info| get_status(info))
        .unwrap_or(FsctStatus::Unknown);

    let timeline_properties = session
        .GetTimelineProperties()
        .into_player_error()
        .inspect_err(|e| error!("[WindowsPlayer] Failed to get timeline properties: {:?}", e))
        .ok();
    let timeline = timeline_properties
        .as_ref()
        .map(|timeline_properties| {
            get_timeline_info(playback_info.as_ref(), timeline_properties)
                .inspect_err(|e| debug!("[WindowsPlayer] Failed to get timeline: {:?}", e))
                .ok()
        })
        .flatten()
        .flatten();

    let texts = get_texts_from_session(session)
        .await
        .inspect_err(|e| error!("[WindowsPlayer] Failed to get media properties: {:?}", e))
        .unwrap_or_default();

    Ok(PlayerState {
        status,
        timeline,
        texts,
    })
}

trait IntoPlayerResult<T> {
    fn into_player_error(self) -> Result<T, PlayerError>;
}

impl<T> IntoPlayerResult<T> for Result<T, WindowsError> {
    fn into_player_error(self) -> Result<T, PlayerError> {
        self.map_err(|e| PlayerError::Other(e.into()))
    }
}

struct WindowsSessionHandles {
    session: GlobalSystemMediaTransportControlsSession,
    playback_info_change_registration_handle: i64,
    timeline_properties_changed_registration_handle: i64,
    media_properties_changed_registration_handle: i64,
}

impl WindowsSessionHandles {
    fn new(
        session: GlobalSystemMediaTransportControlsSession,
        notification_tx: tokio::sync::mpsc::Sender<WindowsNotification>,
    ) -> Result<WindowsSessionHandles, PlayerError> {
        debug!("[WindowsPlayer] Creating session handles");
        let playback_info_changed_notification_tx = notification_tx.clone();
        let playback_info_changed_handler = TypedEventHandler::<
            GlobalSystemMediaTransportControlsSession,
            PlaybackInfoChangedEventArgs,
        >::new(move |session, _event_args| -> windows_core::Result<()> {
            debug!("[WindowsPlayer] Playback info changed handler called");
            playback_info_changed_notification_tx
                .blocking_send(WindowsNotification::SessionNotification {
                    topic: SessionNotificationTopic::PlaybackInfoChanged,
                    session: session.clone(),
                })
                .map_err(|_| WindowsError::empty())
        });

        let timeline_properties_changed_notification_tx = notification_tx.clone();
        let timeline_properties_changed_handler =
            TypedEventHandler::<GlobalSystemMediaTransportControlsSession, TimelinePropertiesChangedEventArgs>::new(
                move |session, _event_args| -> windows_core::Result<()> {
                    debug!("[WindowsPlayer] Timeline properties changed handler called");
                    timeline_properties_changed_notification_tx
                        .blocking_send(WindowsNotification::SessionNotification {
                            topic: SessionNotificationTopic::TimelinePropertiesChanged,
                            session: session.clone(),
                        })
                        .map_err(|_| WindowsError::empty())
                },
            );

        let media_properties_changed_notification_tx = notification_tx;
        let media_properties_changed_handler =
            TypedEventHandler::<GlobalSystemMediaTransportControlsSession, MediaPropertiesChangedEventArgs>::new(
                move |session, _event_args| -> windows_core::Result<()> {
                    debug!("[WindowsPlayer] Media properties changed handler called");
                    media_properties_changed_notification_tx
                        .blocking_send(WindowsNotification::SessionNotification {
                            topic: SessionNotificationTopic::MediaPropertiesChanged,
                            session: session.clone(),
                        })
                        .map_err(|_| WindowsError::empty())
                },
            );

        let playback_info_change_registration_result = session
            .PlaybackInfoChanged(&playback_info_changed_handler)
            .into_player_error();

        let timeline_properties_changed_registration_result = session
            .TimelinePropertiesChanged(&timeline_properties_changed_handler)
            .into_player_error();

        let media_properties_changed_registration_result = session
            .MediaPropertiesChanged(&media_properties_changed_handler)
            .into_player_error();

        if playback_info_change_registration_result.is_err()
            || timeline_properties_changed_registration_result.is_err()
            || media_properties_changed_registration_result.is_err()
        {
            warn!("[WindowsPlayer] Failed to register to session");

            if let Ok(playback_info_change_registration_handle) = playback_info_change_registration_result {
                session
                    .RemovePlaybackInfoChanged(playback_info_change_registration_handle)
                    .into_player_error()
                    .ok();
            }
            if let Ok(timeline_properties_changed_registration_handle) = timeline_properties_changed_registration_result
            {
                session
                    .RemoveTimelinePropertiesChanged(timeline_properties_changed_registration_handle)
                    .into_player_error()
                    .ok();
            }
            if let Ok(media_properties_changed_registration_handle) = media_properties_changed_registration_result {
                session
                    .RemoveMediaPropertiesChanged(media_properties_changed_registration_handle)
                    .into_player_error()
                    .ok();
            }

            return Err(PlayerError::PermissionDenied);
        }

        let playback_info_change_registration_handle = playback_info_change_registration_result.unwrap();
        let timeline_properties_changed_registration_handle = timeline_properties_changed_registration_result.unwrap();
        let media_properties_changed_registration_handle = media_properties_changed_registration_result.unwrap();

        let handles = WindowsSessionHandles {
            session,
            playback_info_change_registration_handle,
            timeline_properties_changed_registration_handle,
            media_properties_changed_registration_handle,
        };
        debug!("[WindowsPlayer] Session handles created");
        Ok(handles)
    }
}

impl Drop for WindowsSessionHandles {
    fn drop(&mut self) {
        self.session
            .RemovePlaybackInfoChanged(self.playback_info_change_registration_handle)
            .ok();
        self.session
            .RemoveTimelinePropertiesChanged(self.timeline_properties_changed_registration_handle)
            .ok();
        self.session
            .RemoveMediaPropertiesChanged(self.media_properties_changed_registration_handle)
            .ok();
        debug!("[WindowsPlayer] Session handles dropped");
    }
}

struct WindowsOsWatcher {
    driver: Arc<dyn FsctDriver>,
    player_id: ManagedPlayerId,
    handles: Mutex<Option<WindowsSessionHandles>>,
    state: Mutex<CombinedState>,
}

async fn get_session_manager() -> Result<GlobalSystemMediaTransportControlsSessionManager, PlayerError> {
    let session_manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .into_player_error()?
        .await
        .into_player_error()?;
    Ok(session_manager)
}

impl WindowsOsWatcher {
    async fn new_with_driver(driver: Arc<dyn FsctDriver>) -> Result<Self, PlayerError> {
        let player_id = driver
            .register_player("native-windows-gsmtc".to_string())
            .await
            .map_err(|e| PlayerError::Other(e.into()))?;
        Ok(WindowsOsWatcher {
            driver,
            player_id,
            handles: Mutex::new(None),
            state: Mutex::new(CombinedState::default()),
        })
    }

    async fn init_session_manager(
        &self,
        session_manager: &GlobalSystemMediaTransportControlsSessionManager,
        notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>,
    ) -> Result<(), PlayerError> {
        let current_session_change_event_handler =
            TypedEventHandler::<GlobalSystemMediaTransportControlsSessionManager, CurrentSessionChangedEventArgs>::new(
                move |session_manager, _event_args| -> windows_core::Result<()> {
                    debug!("[WindowsPlayer] Current session changed handler called");
                    notification_sender
                        .blocking_send(WindowsNotification::CurrentSessionChanged(session_manager.clone()))
                        .ok();
                    Ok(())
                },
            );

        session_manager
            .CurrentSessionChanged(&current_session_change_event_handler)
            .into_player_error()?;

        Ok(())
    }

    async fn try_update_current_session(
        &self,
        session_manager: Option<&GlobalSystemMediaTransportControlsSessionManager>,
        notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>,
    ) -> Result<(), PlayerError> {
        let session_manager = session_manager.ok_or(PlayerError::PermissionDenied)?;
        let session = session_manager
            .GetCurrentSession()
            .inspect_err(|e| {
                if e.code() != HRESULT(0) {
                    error!("[WindowsPlayer] Can't get current session, error: {:?}", e)
                }
            })
            .into_player_error()?;
        debug!("[WindowsPlayer] Current session: {:?}", session);
        let new_player_state = get_playback_state(&session).await?;
        debug!("[WindowsPlayer] New player state: {:?}", new_player_state);
        // Seed the single source of truth before handles start delivering events,
        // so the first incremental event gates its emit against the initial timeline.
        *self.state.lock().unwrap() = read_combined_state(&session);
        self.handles.lock().unwrap().take();
        *self.handles.lock().unwrap() = Some(WindowsSessionHandles::new(session, notification_sender)?);
        self.driver
            .update_player_state(self.player_id, new_player_state)
            .await
            .map_err(|e| PlayerError::Other(e.into()))?;
        Ok(())
    }

    async fn update_current_session(
        &self,
        session_manager: Option<&GlobalSystemMediaTransportControlsSessionManager>,
        notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>,
    ) {
        if self
            .try_update_current_session(session_manager, notification_sender)
            .await
            .is_err()
        {
            debug!("[WindowsPlayer] Cannot init current session, resetting state");
            let _ = self
                .driver
                .update_player_state(self.player_id, PlayerState::default())
                .await;
        }
    }

    fn is_current_session(&self, session: &GlobalSystemMediaTransportControlsSession) -> bool {
        let handles = self.handles.lock().unwrap();
        if handles.is_none() {
            return false;
        }
        let handles = handles.as_ref().unwrap();
        *session == handles.session
    }
    async fn run_notification_task(self: Arc<Self>) -> Result<JoinableTaskHandle, PlayerError> {
        let (startup_done_signal, startup_awaiter) = tokio::sync::oneshot::channel::<()>();
        let service_handle = spawn_service(move |mut stop_token| async move {
            debug!("[WindowsPlayer] Notification task started");
            // it is important to create and leave session_manager in this task forever in order not to lose notifications
            let session_manager = get_session_manager().await;
            if session_manager.is_err() {
                debug!("[WindowsPlayer] Failed to get session manager");
                startup_done_signal.send(()).unwrap_or_default();
                return;
            }
            let (notification_sender, mut notification_receiver) =
                tokio::sync::mpsc::channel::<WindowsNotification>(100);

            let session_manager = session_manager.unwrap();
            if self
                .init_session_manager(&session_manager, notification_sender.clone())
                .await
                .is_err()
            {
                debug!("[WindowsPlayer] Failed to init session manager");
                startup_done_signal.send(()).unwrap_or_default();
                return;
            }
            self.update_current_session(Some(&session_manager), notification_sender.clone())
                .await;
            startup_done_signal.send(()).unwrap_or_default();

            while let Some(notification) = tokio::select! {
                Some(n) = notification_receiver.recv() => Some(n),
                _ = stop_token.signaled() => None,
            } {
                match notification {
                    WindowsNotification::CurrentSessionChanged(session_manager) => {
                        debug!("[WindowsPlayer] Current session changed");
                        self.update_current_session(session_manager.as_ref(), notification_sender.clone())
                            .await;
                    }
                    WindowsNotification::SessionNotification { topic, session } => {
                        debug!("[WindowsPlayer] Session notification");
                        self.handle_session_notification(topic, session).await;
                    }
                }
            }
            debug!("[WindowsPlayer] Notification task stopped");
        });
        startup_awaiter.await.map_err(|_| PlayerError::PermissionDenied)?;
        Ok(service_handle)
    }

    async fn handle_session_notification(
        &self,
        topic: SessionNotificationTopic,
        session: Option<GlobalSystemMediaTransportControlsSession>,
    ) {
        if let Some(session) = session {
            if !self.is_current_session(&session) {
                return;
            }
            match topic {
                SessionNotificationTopic::PlaybackInfoChanged => {
                    debug!("[WindowsPlayer] Playback info changed");
                    self.handle_playback_info_changed(session).await;
                }
                SessionNotificationTopic::TimelinePropertiesChanged => {
                    debug!("[WindowsPlayer] Timeline properties changed");
                    self.handle_timeline_properties_changed(session).await;
                }
                SessionNotificationTopic::MediaPropertiesChanged => {
                    debug!("[WindowsPlayer] Media properties changed");
                    self.handle_media_properties_changed(session).await;
                }
            }
        }
    }

    async fn handle_media_properties_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        // Partial update: update only text metadata fields that we can fetch
        if let Ok(texts) = get_texts_from_session(&session).await {
            for meta_id in texts.iter_id() {
                let value = texts.get_text(*meta_id).clone();
                let _ = self
                    .driver
                    .update_player_metadata(self.player_id, *meta_id, value)
                    .await;
            }
        }
    }

    async fn handle_timeline_properties_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        // Update the timeline parts in the combined state, then recompute the emitted
        // timeline's rate from the *stored* status (not a freshly-fetched one) so the two
        // GSMTC events can't race on the rate.
        let Ok(tprops) = session.GetTimelineProperties().into_player_error() else {
            return;
        };
        let Ok(parts) = get_timeline_parts(&tprops) else {
            return;
        };
        let timeline = {
            let mut state = self.state.lock().unwrap();
            state.parts = Some(parts);
            Self::recompute_timeline(&mut state)
        };
        if let Some(timeline) = timeline {
            let _ = self.driver.update_player_timeline(self.player_id, timeline).await;
        }
    }

    async fn handle_playback_info_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        // Update status + reported rate in the combined state, then recompute the timeline
        // so the device's extrapolation rate flips in lockstep with the status. Without this
        // the rate lagged the status by one transition (advanced on pause / froze on play).
        let Ok(info) = session.GetPlaybackInfo().into_player_error() else {
            return;
        };
        let status = get_status(&info);
        let reported_rate = get_reported_rate(&info);
        let timeline = {
            let mut state = self.state.lock().unwrap();
            // re-anchor position to now using the old rate before the status changes
            Self::recalculate_position(&mut state);
            state.status = status;
            state.reported_rate = reported_rate;
            Self::recompute_timeline(&mut state)
        };
        let _ = self.driver.update_player_status(self.player_id, status).await;
        if let Some(timeline) = timeline {
            let _ = self.driver.update_player_timeline(self.player_id, timeline).await;
        }
    }

    /// Rebuild the timeline from the combined state and gate it against the last emitted one.
    /// Returns `Some` only when it changed (the inner `Option` is the value to send).
    fn recompute_timeline(state: &mut CombinedState) -> Option<Option<TimelineInfo>> {
        let timeline = build_timeline(state);
        if timeline != state.last_timeline {
            state.last_timeline = timeline.clone();
            Some(timeline)
        } else {
            None
        }
    }

    /// Advance the stored position to now using the current (gated) rate, then re-anchor
    /// the update time. Mirrors the Linux port so a status change freezes/resumes from the
    /// correct point instead of an old timestamp.
    fn recalculate_position(state: &mut CombinedState) {
        let rate = effective_rate(state.status, state.reported_rate);
        if let Some(parts) = state.parts.as_mut() {
            let now = Instant::now();
            let elapsed = now.saturating_duration_since(parts.update_time);
            let advance = (elapsed.as_secs_f64() * rate).max(0.0);
            parts.position += Duration::from_secs_f64(advance);
            parts.update_time = now;
        }
    }
}

enum SessionNotificationTopic {
    PlaybackInfoChanged,
    TimelinePropertiesChanged,
    MediaPropertiesChanged,
}

enum WindowsNotification {
    CurrentSessionChanged(Option<GlobalSystemMediaTransportControlsSessionManager>),
    SessionNotification {
        topic: SessionNotificationTopic,
        session: Option<GlobalSystemMediaTransportControlsSession>,
    },
}

const UNIX_EPOCH_OFFSET: i64 = 116444736000000000;

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> Result<JoinableTaskHandle, PlayerError> {
    let windows_watcher = Arc::new(WindowsOsWatcher::new_with_driver(driver).await?);
    windows_watcher.run_notification_task().await
}
