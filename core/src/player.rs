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

use crate::definitions::FsctStatus;
use crate::definitions::*;
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;
use log::debug;

use super::player_state::*;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("Permission denied")]
    PermissionDenied,

    #[error("Feature not supported")]
    FeatureNotSupported,

    #[error("Player not found")]
    PlayerNotFound,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, PartialEq, Clone)]
pub enum PlayerEvent {
    StatusChanged(FsctStatus),
    TextChanged((FsctTextMetadata, Option<String>)),
    TimelineChanged(Option<TimelineInfo>),
}

pub type PlayerEventsReceiver = tokio::sync::broadcast::Receiver<PlayerEvent>;
pub type PlayerEventsSender = tokio::sync::broadcast::Sender<PlayerEvent>;

pub type PlayerEventReceiveError = tokio::sync::broadcast::error::RecvError;
pub type PlayerEventSendError = tokio::sync::broadcast::error::SendError<PlayerEvent>;

const DEFAULT_CAPACITY: usize = 100;

pub fn create_player_events_channel() -> (PlayerEventsSender, PlayerEventsReceiver) {
    tokio::sync::broadcast::channel(DEFAULT_CAPACITY)
}

#[async_trait]
pub trait PlayerInterface: Send + Sync {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn play(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn pause(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn stop(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn next_track(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
    async fn previous_track(&self) -> Result<(), PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        Err(PlayerError::FeatureNotSupported)
    }
}

#[derive(Clone)]
pub struct Player {
    player_impl: Arc<dyn PlayerInterface + Sync + Send>,
}

impl Player {
    pub fn new<T: PlayerInterface + Sync + Send + 'static>(player_impl: T) -> Self {
        Self {
            player_impl: Arc::new(player_impl),
        }
    }

    pub fn from_arc(player_impl: Arc<dyn PlayerInterface + Sync + Send>) -> Self {
        Self { player_impl }
    }
}

#[async_trait]
impl PlayerInterface for Player {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        self.player_impl.get_current_state().await
    }
    async fn play(&self) -> Result<(), PlayerError> {
        self.player_impl.play().await
    }
    async fn pause(&self) -> Result<(), PlayerError> {
        self.player_impl.pause().await
    }
    async fn stop(&self) -> Result<(), PlayerError> {
        self.player_impl.stop().await
    }
    async fn next_track(&self) -> Result<(), PlayerError> {
        self.player_impl.next_track().await
    }
    async fn previous_track(&self) -> Result<(), PlayerError> {
        self.player_impl.previous_track().await
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        self.player_impl.listen_to_player_notifications().await
    }
}

pub fn send_all_changed(state: &PlayerState, tx: &PlayerEventsSender) {
    debug!("Sending all player state change events");
    debug!("Sending event TextChanged(CurrentTitle, {}) ", state.texts.title.as_ref().map(|s| s.as_str()).unwrap_or("None"));
    tx.send(PlayerEvent::TextChanged((
        FsctTextMetadata::CurrentTitle,
        state.texts.title.clone(),
    )))
      .unwrap_or_default();
    debug!("Sending event TextChanged(CurrentAuthor, {}) ", state.texts.artist.as_ref().map(|s| s.as_str()).unwrap_or("None"));
    tx.send(PlayerEvent::TextChanged((
        FsctTextMetadata::CurrentAuthor,
        state.texts.artist.clone(),
    )))
      .unwrap_or_default();
    debug!("Sending event TextChanged(CurrentAlbum, {}) ", state.texts.album.as_ref().map(|s| s.as_str()).unwrap_or("None"));
    tx.send(PlayerEvent::TextChanged((
        FsctTextMetadata::CurrentAlbum,
        state.texts.album.clone(),
    )))
      .unwrap_or_default();
    debug!("Sending event StatusChanged({:?}) ", state.status);
    tx.send(PlayerEvent::StatusChanged(state.status))
      .unwrap_or_default();
    debug!("Sending event TimelineChanged({:?}) ", state.timeline.as_ref());
    tx.send(PlayerEvent::TimelineChanged(state.timeline.clone()))
      .unwrap_or_default();
}