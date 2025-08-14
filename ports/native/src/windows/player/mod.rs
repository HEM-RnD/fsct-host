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

use std::sync::{Arc, Mutex};
use std::time::Duration;
use log::{debug, error, warn};
use windows::{
    core::Error as WindowsError,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
    },
};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSessionMediaProperties, GlobalSystemMediaTransportControlsSessionPlaybackInfo, GlobalSystemMediaTransportControlsSessionTimelineProperties, MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, TimelinePropertiesChangedEventArgs};
use fsct_core::definitions::{TimelineInfo, FsctStatus};
use fsct_core::player_state::{PlayerState, TrackMetadata};
use fsct_core::{spawn_service, FsctDriver, ManagedPlayerId, ServiceHandle};
use anyhow::Error as AnyError;
use windows_core::HRESULT;

#[derive(Debug)]
pub enum PlayerError {
    PermissionDenied,
    PlayerNotFound,
    Other(AnyError),
}

fn get_timeline_info(playback_info: Option<&GlobalSystemMediaTransportControlsSessionPlaybackInfo>,
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


async fn get_texts_from_session(session: &GlobalSystemMediaTransportControlsSession) -> Result<TrackMetadata, PlayerError> {
    let media_properties = session.TryGetMediaPropertiesAsync().into_player_error()?.await.into_player_error()?;
    Ok(get_texts(&media_properties))
}

fn get_rate(playback_info: Option<&GlobalSystemMediaTransportControlsSessionPlaybackInfo>) -> f64 {
    if let Some(playback_info) = playback_info {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
        if playback_info.PlaybackStatus().unwrap_or(PlaybackStatus::Closed) != PlaybackStatus::Playing {
            return 0.0;
        }
        playback_info.PlaybackRate().map(|rate| rate.Value().unwrap_or(1.0)).unwrap_or(1.0)
    } else {
        0.0
    }
}

async fn get_playback_state(session: &GlobalSystemMediaTransportControlsSession) -> Result<PlayerState, PlayerError> {
    let playback_info = session.GetPlaybackInfo().into_player_error()
                               .inspect_err(|e| error!("[WindowsPlayer] Failed to get playback info: {:?}", e)).ok();
    let status = playback_info.as_ref().map(|info| get_status(info)).unwrap_or(FsctStatus::Unknown);

    let timeline_properties = session.GetTimelineProperties().into_player_error()
                                     .inspect_err(|e| error!("[WindowsPlayer] Failed to get timeline properties: {:?}", e)).ok();
    let timeline = timeline_properties.as_ref().map(|timeline_properties|
        get_timeline_info(playback_info.as_ref(), timeline_properties).inspect_err(|e| debug!("[WindowsPlayer] Failed to get timeline: {:?}", e)).ok()).flatten().flatten();

    let texts = get_texts_from_session(session).await.inspect_err(|e| error!("[WindowsPlayer] Failed to get media properties: {:?}", e)).unwrap_or_default();

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
    fn new(session: GlobalSystemMediaTransportControlsSession, notification_tx: tokio::sync::mpsc::Sender<WindowsNotification>)
        -> Result<WindowsSessionHandles, PlayerError> {
        debug!("[WindowsPlayer] Creating session handles");
        let playback_info_changed_notification_tx = notification_tx.clone();
        let playback_info_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
            PlaybackInfoChangedEventArgs>::new(move
            |session, _event_args| -> windows_core::Result<()> {
            debug!("[WindowsPlayer] Playback info changed handler called");
            playback_info_changed_notification_tx.blocking_send(WindowsNotification::SessionNotification {
                topic: SessionNotificationTopic::PlaybackInfoChanged,
                session: session.clone(),
            }).map_err(|_| WindowsError::empty())
        });


        let timeline_properties_changed_notification_tx = notification_tx.clone();
        let timeline_properties_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
            TimelinePropertiesChangedEventArgs>::new(move |session, _event_args| -> windows_core::Result<()> {
            debug!("[WindowsPlayer] Timeline properties changed handler called");
            timeline_properties_changed_notification_tx.blocking_send(WindowsNotification::SessionNotification {
                topic: SessionNotificationTopic::TimelinePropertiesChanged,
                session: session.clone(),
            }).map_err(|_| WindowsError::empty())
        });

        let media_properties_changed_notification_tx = notification_tx;
        let media_properties_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
            MediaPropertiesChangedEventArgs>::new(move |session, _event_args| -> windows_core::Result<()> {
            debug!("[WindowsPlayer] Media properties changed handler called");
            media_properties_changed_notification_tx.blocking_send(WindowsNotification::SessionNotification {
                topic: SessionNotificationTopic::MediaPropertiesChanged,
                session: session.clone(),
            }).map_err(|_| WindowsError::empty())
        });


        let playback_info_change_registration_result = session.PlaybackInfoChanged(&playback_info_changed_handler)
                                                              .into_player_error();

        let timeline_properties_changed_registration_result = session.TimelinePropertiesChanged(&timeline_properties_changed_handler)
                                                                     .into_player_error();

        let media_properties_changed_registration_result = session.MediaPropertiesChanged(&media_properties_changed_handler)
                                                                  .into_player_error();

        if playback_info_change_registration_result.is_err() || timeline_properties_changed_registration_result.is_err() || media_properties_changed_registration_result.is_err() {
            warn!("[WindowsPlayer] Failed to register to session");

            if let Ok(playback_info_change_registration_handle) = playback_info_change_registration_result {
                session.RemovePlaybackInfoChanged(playback_info_change_registration_handle).into_player_error().ok();
            }
            if let Ok(timeline_properties_changed_registration_handle) = timeline_properties_changed_registration_result {
                session.RemoveTimelinePropertiesChanged(timeline_properties_changed_registration_handle).into_player_error().ok();
            }
            if let Ok(media_properties_changed_registration_handle) = media_properties_changed_registration_result {
                session.RemoveMediaPropertiesChanged(media_properties_changed_registration_handle).into_player_error().ok();
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
        self.session.RemovePlaybackInfoChanged(self.playback_info_change_registration_handle).ok();
        self.session.RemoveTimelinePropertiesChanged(self.timeline_properties_changed_registration_handle).ok();
        self.session.RemoveMediaPropertiesChanged(self.media_properties_changed_registration_handle).ok();
        debug!("[WindowsPlayer] Session handles dropped");
    }
}

struct WindowsOsWatcher {
    driver: Arc<dyn FsctDriver>,
    player_id: ManagedPlayerId,
    handles: Mutex<Option<WindowsSessionHandles>>,
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
        let player_id = driver.register_player("native-windows-gsmtc".to_string()).await.map_err(|e| PlayerError::Other(e.into()))?;
        Ok(WindowsOsWatcher {
            driver,
            player_id,
            handles: Mutex::new(None),
        })
    }

    fn get_session(&self) -> Result<GlobalSystemMediaTransportControlsSession, PlayerError> {
        Ok(self.handles.lock().unwrap().as_ref().ok_or(PlayerError::PlayerNotFound)?.session.clone())
    }

    async fn init_session_manager(&self, session_manager: &GlobalSystemMediaTransportControlsSessionManager,
                                  notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>) -> Result<(),
        PlayerError> {
        let current_session_change_event_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSessionManager,
            CurrentSessionChangedEventArgs>::new(move |session_manager, _event_args| -> windows_core::Result<()> {
            debug!("[WindowsPlayer] Current session changed handler called");
            notification_sender.blocking_send(WindowsNotification::CurrentSessionChanged(session_manager.clone())).ok();
            Ok(())
        });

        session_manager.CurrentSessionChanged(&current_session_change_event_handler).into_player_error()?;

        Ok(())
    }


    async fn try_update_current_session(&self,
                                        session_manager: Option<&GlobalSystemMediaTransportControlsSessionManager>,
                                        notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>) -> Result<(), PlayerError> {
        let session_manager = session_manager.ok_or(PlayerError::PermissionDenied)?;
        let session = session_manager
            .GetCurrentSession()
            .inspect_err(|e|
                if e.code() != HRESULT(0) {
                    error!("[WindowsPlayer] Can't get current session, error: {:?}",e)
                }
            )
            .into_player_error()?;
        debug!("[WindowsPlayer] Current session: {:?}", session);
        let new_player_state = get_playback_state(&session).await?;
        debug!("[WindowsPlayer] New player state: {:?}", new_player_state);
        self.handles.lock().unwrap().take();
        *self.handles.lock().unwrap() = Some(WindowsSessionHandles::new(session, notification_sender)?);
        self.driver.update_player_state(self.player_id, new_player_state).await.map_err(|e| PlayerError::Other(e.into()))?;
        Ok(())
    }

    async fn update_current_session(&self,
                                    session_manager: Option<&GlobalSystemMediaTransportControlsSessionManager>,
                                    notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>) {
        if self.try_update_current_session(session_manager, notification_sender).await.is_err() {
            debug!("[WindowsPlayer] Cannot init current session, resetting state");
            let _ = self.driver.update_player_state(self.player_id, PlayerState::default()).await;
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
    async fn run_notification_task(self: Arc<Self>) -> Result<ServiceHandle, PlayerError> {
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
            let (notification_sender, mut notification_receiver) = tokio::sync::mpsc::channel::<WindowsNotification>(100);

            let session_manager = session_manager.unwrap();
            if self.init_session_manager(&session_manager, notification_sender.clone()).await.is_err() {
                debug!("[WindowsPlayer] Failed to init session manager");
                startup_done_signal.send(()).unwrap_or_default();
                return;
            }
            self.update_current_session(Some(&session_manager), notification_sender.clone()).await;
            startup_done_signal.send(()).unwrap_or_default();

            while let Some(notification) = tokio::select! {
                                                                Some(n) = notification_receiver.recv() => Some(n),
                                                                _ = stop_token.signaled() => None,
                                                            }
            {
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

    async fn handle_session_notification(&self, topic: SessionNotificationTopic, session:
    Option<GlobalSystemMediaTransportControlsSession>) {
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
                let _ = self.driver.update_player_metadata(self.player_id, *meta_id, value).await;
            }
        }
    }

    async fn handle_timeline_properties_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        // Partial update: recompute timeline (position, duration, rate)
        let playback_info = session.GetPlaybackInfo().into_player_error().ok();
        let timeline_props = session.GetTimelineProperties().into_player_error().ok();
        if let Some(tprops) = timeline_props {
            if let Ok(Some(timeline)) = get_timeline_info(playback_info.as_ref(), &tprops) {
                let _ = self.driver.update_player_timeline(self.player_id, Some(timeline)).await;
            }
        }
    }

    async fn handle_playback_info_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        // Partial update: update only playback status
        if let Ok(info) = session.GetPlaybackInfo().into_player_error() {
            let status = get_status(&info);
            let _ = self.driver.update_player_status(self.player_id, status).await;
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


pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> Result<ServiceHandle, PlayerError> {
    let windows_watcher = Arc::new(WindowsOsWatcher::new_with_driver(driver).await?);
    windows_watcher.run_notification_task().await
}



