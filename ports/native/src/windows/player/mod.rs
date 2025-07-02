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
use log::{debug, info, warn};
use windows::{
    core::Error as WindowsError,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
    },
};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSessionMediaProperties, GlobalSystemMediaTransportControlsSessionPlaybackInfo, GlobalSystemMediaTransportControlsSessionTimelineProperties, MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, SessionsChangedEventArgs, TimelinePropertiesChangedEventArgs};
use windows_core::Interface;
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
struct WindowsPlayerInternals {
    player_state: PlayerState,
    player_event_tx: PlayerEventsSender,
    notification_tx: tokio::sync::mpsc::Sender<WindowsNotification>,
    handles: Option<WindowsSessionHandles>,

}

pub struct WindowsSystemPlayer {
    internals: Arc<Mutex<WindowsPlayerInternals>>,
}

enum WindowsNotification {
    CurrentSessionChanged(Option<GlobalSystemMediaTransportControlsSessionManager>),
    PlaybackInfoChanged(Option<GlobalSystemMediaTransportControlsSession>),
    TimelinePropertiesChanged(Option<GlobalSystemMediaTransportControlsSession>),
    MediaPropertiesChanged(Option<GlobalSystemMediaTransportControlsSession>),
}


const UNIX_EPOCH_OFFSET: i64 = 116444736000000000;


fn unregister_from_session(handles: WindowsSessionHandles) {
    handles.session.RemovePlaybackInfoChanged(handles.playback_info_change_registration_handle).ok();
    handles.session.RemoveTimelinePropertiesChanged(handles.timeline_properties_changed_registration_handle).ok();
    handles.session.RemoveMediaPropertiesChanged(handles.media_properties_changed_registration_handle).ok();
}

fn register_to_session(session: GlobalSystemMediaTransportControlsSession,
                       notification_tx: tokio::sync::mpsc::Sender<WindowsNotification>)
    -> Result<WindowsSessionHandles, PlayerError> {
    let playback_info_changed_notification_tx = notification_tx.clone();
    let playback_info_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
        PlaybackInfoChangedEventArgs>::new(move
        |session, event_args| -> windows_core::Result<()> {
        debug!("Playback info changed handler called");
        playback_info_changed_notification_tx.blocking_send(WindowsNotification::PlaybackInfoChanged(session.clone())).map_err(|_|
            WindowsError::empty())
    });


    let timeline_properties_changed_notification_tx = notification_tx.clone();
    let timeline_properties_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
        TimelinePropertiesChangedEventArgs>::new(move |session, _event_args| -> windows_core::Result<()> {
        debug!("Timeline properties changed handler called");
        timeline_properties_changed_notification_tx.blocking_send(WindowsNotification::TimelinePropertiesChanged(session.clone())).map_err(|_|
            WindowsError::empty())
    });

    let media_properties_changed_notification_tx = notification_tx;
    let media_properties_changed_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSession,
        MediaPropertiesChangedEventArgs>::new(move |session, _event_args| -> windows_core::Result<()> {
        debug!("Media properties changed handler called");
        media_properties_changed_notification_tx.blocking_send(WindowsNotification::MediaPropertiesChanged(session.clone())).map_err(|_|
            WindowsError::empty())
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
}
async fn try_update_current_session(session_manager: Option<GlobalSystemMediaTransportControlsSessionManager>,
                                    internals: &Arc<Mutex<WindowsPlayerInternals>>) -> Result<(), PlayerError> {
    let session_manager = session_manager.ok_or(PlayerError::PermissionDenied)?;
    let session = session_manager.GetCurrentSession().into_player_error()?;
    let new_player_state = get_playback_state(&session).await?;
    let mut internals_locked = internals.lock().unwrap();
    let handles = internals_locked.handles.take();
    if let Some(handles) = handles {
        unregister_from_session(handles);
    }
    let handles = register_to_session(session, internals_locked.notification_tx.clone())?;
    internals_locked.handles = Some(handles);
    internals_locked.player_state = new_player_state;

    Ok(())
}

async fn update_current_session(session_manager: Option<GlobalSystemMediaTransportControlsSessionManager>,
                                internals: &Arc<Mutex<WindowsPlayerInternals>>) {
    if (try_update_current_session(session_manager, internals).await.is_err()) {
        internals.lock().unwrap().player_state = PlayerState::default();
    }

    let internals_locked = internals.lock().unwrap();
    player::send_all_changed(&internals_locked.player_state, &internals_locked.player_event_tx);
}

async fn get_texts_from_session(session: &GlobalSystemMediaTransportControlsSession) -> Result<TrackMetadata, PlayerError> {
    let media_properties = session.TryGetMediaPropertiesAsync().into_player_error()?.await.into_player_error()?;
    Ok(get_texts(&media_properties))
}

async fn init_session_manager(session_manager: &GlobalSystemMediaTransportControlsSessionManager, internals: &Arc<Mutex<WindowsPlayerInternals>>) -> Result<(),
    PlayerError> {
    let notification_sender = internals.lock().unwrap().notification_tx.clone();
    let current_session_change_event_handler = TypedEventHandler::<GlobalSystemMediaTransportControlsSessionManager,
        CurrentSessionChangedEventArgs>::new(move |session_manager, _event_args| -> windows_core::Result<()> {
        debug!("Current session changed handler called");
        notification_sender.blocking_send(WindowsNotification::CurrentSessionChanged(session_manager.clone())).ok();
        Ok(())
    });

    session_manager.CurrentSessionChanged(&current_session_change_event_handler).into_player_error()?;

    Ok(())
}

async fn get_session_manager() -> Result<GlobalSystemMediaTransportControlsSessionManager, PlayerError> {
    let session_manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .into_player_error()?
        .await
        .into_player_error()?;
    Ok(session_manager)
}
async fn run_notification_task(mut notification_receiver: tokio::sync::mpsc::Receiver<WindowsNotification>,
                               internals: Arc<Mutex<WindowsPlayerInternals>>) -> Result<(), PlayerError> {
    let (oneshot_tx, oneshot_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        debug!("Notification task started");
        let session_manager= get_session_manager().await;
        if session_manager.is_err() {
            debug!("Failed to get session manager");
            oneshot_tx.send(()).unwrap_or_default();
            return;
        }
        let session_manager = session_manager.unwrap();
        if init_session_manager(&session_manager, &internals).await.is_err() {
            debug!("Failed to init session manager");
            oneshot_tx.send(()).unwrap_or_default();
            return;
        }
        update_current_session(Some(session_manager.clone()), &internals).await;
        oneshot_tx.send(()).unwrap_or_default();

        while let Some(notification) = notification_receiver.recv().await {
            match notification {
                WindowsNotification::CurrentSessionChanged(session_manager) => {
                    debug!("Current session changed");
                    update_current_session(session_manager, &internals).await;
                }
                WindowsNotification::PlaybackInfoChanged(session) => {
                    debug!("Playback info changed");
                    if let Some(session) = session {
                        if session != internals.lock().unwrap().handles.as_ref().unwrap().session {
                            continue;
                        }
                        let playback_info = session.GetPlaybackInfo().ok();
                        if let Some(playback_info) = playback_info {
                            let status = get_status(&playback_info);
                            let rate = get_rate(&playback_info);
                            let mut internals_locked = internals.lock().unwrap();
                            let player_event_tx = internals_locked.player_event_tx.clone();
                            let player_state = &mut internals_locked.player_state;
                            if player_state.status != status {
                                player_state.status = status;
                                player_event_tx.send(PlayerEvent::StatusChanged(status)).unwrap_or_default();
                            }
                            if let Some(timeline) = &mut player_state.timeline {
                                timeline.rate = rate;
                                player_event_tx.send(PlayerEvent::TimelineChanged(Some(timeline.clone()))).unwrap_or_default();
                            }
                        }
                    }
                }
                WindowsNotification::MediaPropertiesChanged(session) => {
                    debug!("Media properties changed");
                    if let Some(session) = session {
                        if session != internals.lock().unwrap().handles.as_ref().unwrap().session {
                            continue;
                        }
                        if let Some(texts) = get_texts_from_session(&session).await.ok() {
                            let mut internals_locked = internals.lock().unwrap();
                            let player_event_tx = internals_locked.player_event_tx.clone();
                            let player_state = &mut internals_locked.player_state;
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
                }
                WindowsNotification::TimelinePropertiesChanged(session) => {
                    debug!("Timeline properties changed");
                    if let Some(session) = session {
                        if session != internals.lock().unwrap().handles.as_ref().unwrap().session {
                            continue;
                        }
                        let playback_info = session.GetPlaybackInfo().ok();
                        let timeline_properties = session.GetTimelineProperties().ok();
                        if playback_info.is_none() || timeline_properties.is_none() {
                            let mut internals_locked = internals.lock().unwrap();
                            internals_locked.player_state.timeline = None;
                            internals_locked.player_event_tx.send(PlayerEvent::TimelineChanged(None)).unwrap_or_default();
                            continue;
                        }
                        let playback_info = playback_info.unwrap();
                        let timeline_properties = timeline_properties.unwrap();
                        let timeline = get_timeline_info(&playback_info, &timeline_properties).ok().flatten();
                        let mut internals_locked = internals.lock().unwrap();
                        let player_event_tx = internals_locked.player_event_tx.clone();
                        let player_state = &mut internals_locked.player_state;
                        if timeline == player_state.timeline {
                            continue;
                        }
                        player_state.timeline = timeline.clone();
                        player_event_tx.send(PlayerEvent::TimelineChanged(timeline.clone())).unwrap_or_default();
                    }
                }
            }
        }
        debug!("Notification task stopped");
    });
    oneshot_rx.await.map_err(|_| PlayerError::PermissionDenied)
}

impl WindowsSystemPlayer {
    pub async fn new() -> Result<Self, PlayerError> {


        let (notification_sender, notification_receiver) = tokio::sync::mpsc::channel::<WindowsNotification>(100);

        let (player_event_tx, _) = create_player_events_channel();

        let internals = Arc::new(Mutex::new(WindowsPlayerInternals {
            player_state: PlayerState::default(),
            player_event_tx,
            notification_tx: notification_sender.clone(),
            handles: None,
        }));

        // update_current_session(Some(session_manager.clone()), &internals).await;
        // notification_sender.send(WindowsNotification::CurrentSessionChanged(Some(session_manager.clone()))).await.unwrap();

        run_notification_task(notification_receiver, internals.clone()).await?;

        Ok(Self { internals })
    }

    fn get_session(&self) -> Result<GlobalSystemMediaTransportControlsSession, PlayerError> {
        Ok(self.internals.lock().unwrap().handles.as_ref().ok_or(PlayerError::PlayerNotFound)?.session.clone())
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
        Ok(self.internals.lock().unwrap().player_state.clone())
    }
    async fn play(&self) -> Result<(), PlayerError> {
        self.get_session()?.TryPlayAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn pause(&self) -> Result<(), PlayerError> {
        self.get_session()?.TryPauseAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn stop(&self) -> Result<(), PlayerError> {
        self.get_session()?.TryStopAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn next_track(&self) -> Result<(), PlayerError> {
        self.get_session()?.TrySkipNextAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn previous_track(&self) -> Result<(), PlayerError> {
        self.get_session()?.TrySkipPreviousAsync().into_player_error()?.await.into_player_error()?;
        Ok(())
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        Ok(self.internals.lock().unwrap().player_event_tx.subscribe())
    }
}

pub async fn initialize_native_platform_player() -> anyhow::Result<Player> {
    let windows_player = WindowsSystemPlayer::new().await?;
    Ok(Player::new(windows_player))
}