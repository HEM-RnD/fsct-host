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
use async_trait::async_trait;
use log::{debug, warn};
use windows::{
    core::Error as WindowsError,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
    },
};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSessionMediaProperties, GlobalSystemMediaTransportControlsSessionPlaybackInfo, GlobalSystemMediaTransportControlsSessionTimelineProperties, MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, TimelinePropertiesChangedEventArgs};
use fsct_core::definitions::{FsctTextMetadata, TimelineInfo};
use fsct_core::player::{create_player_events_channel, PlayerError, PlayerEvent, PlayerEventsReceiver, PlayerEventsSender, PlayerInterface, PlayerState, TrackMetadata};
use fsct_core::definitions::FsctStatus;
use fsct_core::{player, Player};

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
        let playback_info_changed_notification_tx = notification_tx.clone();
        let playback_info_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
            PlaybackInfoChangedEventArgs>::new(move
            |session, _event_args| -> windows_core::Result<()> {
            debug!("Playback info changed handler called");
            playback_info_changed_notification_tx.blocking_send(WindowsNotification::SessionNotification {
                topic: SessionNotificationTopic::PlaybackInfoChanged,
                session: session.clone(),
            }).map_err(|_| WindowsError::empty())
        });


        let timeline_properties_changed_notification_tx = notification_tx.clone();
        let timeline_properties_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
            TimelinePropertiesChangedEventArgs>::new(move |session, _event_args| -> windows_core::Result<()> {
            debug!("Timeline properties changed handler called");
            timeline_properties_changed_notification_tx.blocking_send(WindowsNotification::SessionNotification {
                topic: SessionNotificationTopic::TimelinePropertiesChanged,
                session: session.clone(),
            }).map_err(|_| WindowsError::empty())
        });

        let media_properties_changed_notification_tx = notification_tx;
        let media_properties_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
            MediaPropertiesChangedEventArgs>::new(move |session, _event_args| -> windows_core::Result<()> {
            debug!("Media properties changed handler called");
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
            warn!("Failed to register to session");

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
        Ok(handles)
    }}

impl Drop for WindowsSessionHandles {
    fn drop(&mut self) {
        self.session.RemovePlaybackInfoChanged(self.playback_info_change_registration_handle).ok();
        self.session.RemoveTimelinePropertiesChanged(self.timeline_properties_changed_registration_handle).ok();
        self.session.RemoveMediaPropertiesChanged(self.media_properties_changed_registration_handle).ok();
    }
}

struct WindowsPlayerImplementation {
    player_state: Mutex<PlayerState>,
    player_event_tx: PlayerEventsSender,
    handles: Mutex<Option<WindowsSessionHandles>>
}


async fn get_texts_from_session(session: &GlobalSystemMediaTransportControlsSession) -> Result<TrackMetadata, PlayerError> {
    let media_properties = session.TryGetMediaPropertiesAsync().into_player_error()?.await.into_player_error()?;
    Ok(get_texts(&media_properties))
}


async fn get_session_manager() -> Result<GlobalSystemMediaTransportControlsSessionManager, PlayerError> {
    let session_manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .into_player_error()?
        .await
        .into_player_error()?;
    Ok(session_manager)
}

impl WindowsPlayerImplementation {

    fn new() -> Self {
        let (player_event_tx, _) = create_player_events_channel();
        WindowsPlayerImplementation {
            player_state: Mutex::new(PlayerState::default()),
            player_event_tx,
            handles: Mutex::new(None),
        }
    }

    fn get_session(&self) -> Result<GlobalSystemMediaTransportControlsSession, PlayerError> {
        Ok(self.handles.lock().unwrap().as_ref().ok_or(PlayerError::PlayerNotFound)?.session.clone())
    }

    async fn init_session_manager(&self, session_manager: &GlobalSystemMediaTransportControlsSessionManager,
                                  notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>) -> Result<(),
        PlayerError> {
        let current_session_change_event_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSessionManager,
            CurrentSessionChangedEventArgs>::new(move |session_manager, _event_args| -> windows_core::Result<()> {
            debug!("Current session changed handler called");
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
        let session = session_manager.GetCurrentSession().into_player_error()?;
        let new_player_state = get_playback_state(&session).await?;
        self.handles.lock().unwrap().take();
        *self.player_state.lock().unwrap() = new_player_state;
        *self.handles.lock().unwrap() = Some(WindowsSessionHandles::new(session, notification_sender)?);

        Ok(())
    }

    async fn update_current_session(&self,
                                    session_manager: Option<&GlobalSystemMediaTransportControlsSessionManager>,
                                    notification_sender: tokio::sync::mpsc::Sender<WindowsNotification>) {
        if self.try_update_current_session(session_manager, notification_sender).await.is_err() {
            *self.player_state.lock().unwrap() = PlayerState::default();
        }

        let player_state = self.player_state.lock().unwrap().clone();
        player::send_all_changed(&player_state, &self.player_event_tx);
    }

    fn is_current_session(&self, session: &GlobalSystemMediaTransportControlsSession) -> bool {
        let handles = self.handles.lock().unwrap();
        if handles.is_none() {
            return false;
        }
        let handles = handles.as_ref().unwrap();
        *session == handles.session
    }
    async fn run_notification_task(self: Arc<Self>) -> Result<(), PlayerError> {
        let (oneshot_tx, oneshot_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            debug!("Notification task started");
            // it is important to create and leave session_manager in this task forever in order not to lose notifications
            let session_manager = get_session_manager().await;
            if session_manager.is_err() {
                debug!("Failed to get session manager");
                oneshot_tx.send(()).unwrap_or_default();
                return;
            }
            let (notification_sender, mut notification_receiver) = tokio::sync::mpsc::channel::<WindowsNotification>(100);

            let session_manager = session_manager.unwrap();
            if self.init_session_manager(&session_manager, notification_sender.clone()).await.is_err() {
                debug!("Failed to init session manager");
                oneshot_tx.send(()).unwrap_or_default();
                return;
            }
            self.update_current_session(Some(&session_manager), notification_sender.clone()).await;
            oneshot_tx.send(()).unwrap_or_default();

            while let Some(notification) = notification_receiver.recv().await {
                match notification {
                    WindowsNotification::CurrentSessionChanged(session_manager) => {
                        debug!("Current session changed");
                        self.update_current_session(session_manager.as_ref(), notification_sender.clone())
                            .await;
                    }
                    WindowsNotification::SessionNotification{topic, session} => {
                        debug!("Session notification");
                        self.handle_session_notification(topic, session).await;
                    }
                }
            }
            debug!("Notification task stopped");
        });
        oneshot_rx.await.map_err(|_| PlayerError::PermissionDenied)
    }

    async fn handle_session_notification(&self, topic: SessionNotificationTopic, session:
    Option<GlobalSystemMediaTransportControlsSession>) {
        if let Some(session) = session {
            if !self.is_current_session(&session) {
                return;
            }
            match topic {
                SessionNotificationTopic::PlaybackInfoChanged => {
                    debug!("Playback info changed");
                    self.handle_playback_info_changed(session);
                }
                SessionNotificationTopic::TimelinePropertiesChanged => {
                    debug!("Timeline properties changed");
                    self.handle_timeline_properties_changed(session);
                }
                SessionNotificationTopic::MediaPropertiesChanged => {
                    debug!("Media properties changed");
                    self.handle_media_properties_changed(session).await;
                }
            }
        }
    }

    async fn handle_media_properties_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        if let Some(texts) = get_texts_from_session(&session).await.ok() {
            let player_event_tx = &self.player_event_tx;
            let mut player_state = self.player_state.lock().unwrap();
            if player_state.texts.title != texts.title {
                player_state.texts.title = texts.title.clone();
                player_event_tx.send(PlayerEvent::TextChanged((
                    FsctTextMetadata::CurrentTitle,
                    texts.title,
                ))).unwrap_or_default();
            }
            if player_state.texts.artist != texts.artist {
                player_state.texts.artist = texts.artist.clone();
                player_event_tx.send(PlayerEvent::TextChanged((
                    FsctTextMetadata::CurrentAuthor,
                    texts.artist,
                ))).unwrap_or_default();
            }
            if player_state.texts.album != texts.album {
                player_state.texts.album = texts.album.clone();
                player_event_tx.send(PlayerEvent::TextChanged((
                    FsctTextMetadata::CurrentAlbum,
                    texts.album,
                ))).unwrap_or_default();
            }
        }
    }

    fn handle_timeline_properties_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        let playback_info = session.GetPlaybackInfo().ok();
        let timeline_properties = session.GetTimelineProperties().ok();
        let mut player_state = self.player_state.lock().unwrap();
        if playback_info.is_none() || timeline_properties.is_none() {
            player_state.timeline = None;
            self.player_event_tx.send(PlayerEvent::TimelineChanged(None)).unwrap_or_default();
            return;
        }
        let playback_info = playback_info.unwrap();
        let timeline_properties = timeline_properties.unwrap();
        let timeline = get_timeline_info(&playback_info, &timeline_properties).ok().flatten();

        if timeline == player_state.timeline {
            return;
        }
        player_state.timeline = timeline.clone();
        self.player_event_tx.send(PlayerEvent::TimelineChanged(timeline)).unwrap_or_default();
    }

    fn handle_playback_info_changed(&self, session: GlobalSystemMediaTransportControlsSession) {
        let playback_info = session.GetPlaybackInfo().ok();
        if let Some(playback_info) = playback_info {
            let status = get_status(&playback_info);
            let rate = get_rate(&playback_info);
            let mut player_state = self.player_state.lock().unwrap();
            if player_state.status != status {
                player_state.status = status;
                self.player_event_tx.send(PlayerEvent::StatusChanged(status)).unwrap_or_default();
            }
            if let Some(timeline) = &mut player_state.timeline {
                timeline.rate = rate;
                self.player_event_tx.send(PlayerEvent::TimelineChanged(Some(timeline.clone()))).unwrap_or_default();
            }
        }
    }
}

pub struct WindowsSystemPlayer {
    implementation: Arc<WindowsPlayerImplementation>,
}

enum SessionNotificationTopic {
    PlaybackInfoChanged,
    TimelinePropertiesChanged,
    MediaPropertiesChanged,
}

enum WindowsNotification {
    CurrentSessionChanged(Option<GlobalSystemMediaTransportControlsSessionManager>),
    SessionNotification{
        topic: SessionNotificationTopic,
        session: Option<GlobalSystemMediaTransportControlsSession>,
    }
}


const UNIX_EPOCH_OFFSET: i64 = 116444736000000000;



impl WindowsSystemPlayer {
    pub async fn new() -> Result<Self, PlayerError> {
        let internals = Arc::new(WindowsPlayerImplementation::new());
        internals.clone().run_notification_task().await?;
        Ok(Self { implementation: internals })
    }
}

fn get_timeline_info(playback_info: &GlobalSystemMediaTransportControlsSessionPlaybackInfo,
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

fn get_rate(playback_info: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackInfo) -> f64 {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus;
    if playback_info.PlaybackStatus().unwrap_or(PlaybackStatus::Closed) != PlaybackStatus::Playing {
        return 0.0;
    }
    playback_info.PlaybackRate().map(|rate| rate.Value().unwrap_or(1.0)).unwrap_or(1.0)
}

async fn get_playback_state(session: &GlobalSystemMediaTransportControlsSession) -> Result<PlayerState, PlayerError> {
    let playback_info = session.GetPlaybackInfo().into_player_error()?;
    let timeline_properties = session.GetTimelineProperties().into_player_error()?;
    let media_properties = session.TryGetMediaPropertiesAsync().into_player_error()?.await.into_player_error()?;
    let timeline = get_timeline_info(&playback_info, &timeline_properties)?;
    let status = get_status(&playback_info);
    let texts = get_texts(&media_properties);
    Ok(PlayerState {
        status,
        timeline,
        texts,
    })
}

#[async_trait]
impl PlayerInterface for WindowsSystemPlayer {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        Ok(self.implementation.player_state.lock().unwrap().clone())
    }
    async fn play(&self) -> Result<(), PlayerError> {
        self.implementation.get_session()?.TryPlayAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn pause(&self) -> Result<(), PlayerError> {
        self.implementation.get_session()?.TryPauseAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn stop(&self) -> Result<(), PlayerError> {
        self.implementation.get_session()?.TryStopAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn next_track(&self) -> Result<(), PlayerError> {
        self.implementation.get_session()?.TrySkipNextAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn previous_track(&self) -> Result<(), PlayerError> {
        self.implementation.get_session()?.TrySkipPreviousAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        Ok(self.implementation.player_event_tx.subscribe())
    }
}

pub async fn initialize_native_platform_player() -> anyhow::Result<Player> {
    let windows_player = WindowsSystemPlayer::new().await?;
    Ok(Player::new(windows_player))
}