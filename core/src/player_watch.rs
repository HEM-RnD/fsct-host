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

use crate::definitions::FsctTextMetadata;
use crate::player::{
    Player, PlayerError, PlayerEvent, PlayerEventReceiveError, PlayerEventsReceiver,
    PlayerEventsSender, PlayerInterface, create_player_events_channel,
};
use crate::player_state::PlayerState;
use async_trait::async_trait;

use log::{debug, error, info, warn};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::Error;

#[async_trait]
pub trait PlayerEventListener: Send + Sync + 'static {
    async fn on_event(&self, event: PlayerEvent);
}

pub struct NoopPlayerEventListener;

#[async_trait]
impl PlayerEventListener for NoopPlayerEventListener {
    async fn on_event(&self, _event: PlayerEvent) {}
}

impl NoopPlayerEventListener {
    pub fn new() -> Self {
        Self {}
    }
}

// player watch
fn update_current_status(
    new_state: &PlayerState,
    current_state: &mut PlayerState,
    tx: &PlayerEventsSender,
) {
    if new_state.status != current_state.status {
        current_state.status = new_state.status;
        tx.send(PlayerEvent::StatusChanged(new_state.status.clone()))
          .unwrap_or_default();
    }
}

fn update_timeline(
    new_state: &PlayerState,
    current_state: &mut PlayerState,
    tx: &PlayerEventsSender,
) {
    if new_state.timeline != current_state.timeline {
        current_state.timeline = new_state.timeline.clone();
        tx.send(PlayerEvent::TimelineChanged(new_state.timeline.clone()))
          .unwrap_or_default();
    }
}

fn update_text(
    text_id: FsctTextMetadata,
    new_state: &PlayerState,
    current_state: &mut PlayerState,
    tx: &PlayerEventsSender,
) {
    let new_text = new_state.texts.get_text(text_id);
    let current_text = current_state.texts.get_mut_text(text_id);
    if new_text != current_text {
        *current_text = new_text.clone();
        tx.send(PlayerEvent::TextChanged((text_id, new_text.clone())))
          .unwrap_or_default();
    }
}

fn update_texts(new_state: &PlayerState, current_state: &mut PlayerState, tx: &PlayerEventsSender) {
    current_state.texts.iter_id().for_each(|text_id| {
        update_text(*text_id, new_state, current_state, tx);
    });
}

fn update_current_metadata(
    new_state: &PlayerState,
    current_state: &mut PlayerState,
    tx: &PlayerEventsSender,
) {
    update_current_status(new_state, current_state, tx);
    update_timeline(new_state, current_state, tx);
    update_texts(new_state, current_state, tx);
}

fn create_polling_metadata_watch(player: Player) -> PlayerEventsReceiver {
    let (mut tx, rx) = create_player_events_channel();
    tokio::spawn(async move {
        let mut current_metadata = PlayerState::default();
        let mut last_get_current_state_failed = false;
        loop {
            let state = player.get_current_state().await;
            let state = match state {
                Ok(state) => {
                    if last_get_current_state_failed {
                        last_get_current_state_failed = false;
                        info!("Got state after several failures.");
                    }
                    state
                }
                Err(e) => {
                    if !last_get_current_state_failed {
                        last_get_current_state_failed = true;
                        error!("Failed to get state: {}", e);
                    }
                    debug!("Failed to get state: {}", e);
                    PlayerState::default()
                }
            };

            update_current_metadata(&state, &mut current_metadata, &mut tx);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    rx
}

fn update_current_state_on_event(event: &PlayerEvent, current_state: &mut PlayerState) -> bool {
    match event {
        PlayerEvent::StatusChanged(status) => {
            if *status != current_state.status {
                current_state.status = status.clone();
                debug!("Status changed to {:?}", current_state.status);
                return true;
            }
        }
        PlayerEvent::TimelineChanged(timeline) => {
            if *timeline != current_state.timeline {
                current_state.timeline = timeline.clone();
                debug!("Timeline changed to {:?}", current_state.timeline);
                return true;
            }
        }
        PlayerEvent::TextChanged((text_id, text)) => {
            let current_text = current_state.texts.get_mut_text(*text_id);
            if *text != *current_text {
                *current_text = text.clone();
                debug!("Text {:?} changed to {:?}", text_id, text);
                return true;
            }
        }
    };
    false
}


fn transform_event(event: PlayerEvent) -> PlayerEvent {
    match event {
        PlayerEvent::TimelineChanged(Some(timeline)) => {
            // workaround for a situation where duration is (almost) 0
            if timeline.duration <= Duration::from_millis(100) {
                PlayerEvent::TimelineChanged(None)
            } else {
                PlayerEvent::TimelineChanged(Some(timeline))
            }
        }
        other_event => other_event,
    }
}

async fn process_player_event(
    event: PlayerEvent,
    player_event_listener: &impl PlayerEventListener,
    current_metadata: &Arc<Mutex<PlayerState>>,
) {
    let event = transform_event(event);
    let has_changed = update_current_state_on_event(&event, &mut current_metadata.lock().unwrap());
    if !has_changed {
        return;
    }

    player_event_listener.on_event(event).await;
}

async fn get_playback_notification_stream(
    player: Player,
) -> Result<PlayerEventsReceiver, PlayerError> {
    match player.listen_to_player_notifications().await {
        Ok(listener) => {
            debug!("Player supports notification stream, Using it");
            Ok(listener)
        }
        Err(PlayerError::FeatureNotSupported) => {
            debug!(
                "Player doesn't support notification stream, Using polling metadata watch fallback"
            );
            Ok(create_polling_metadata_watch(player))
        }
        Err(e) => Err(e),
    }
}

pub async fn run_player_watch(
    player: Player,
    player_event_listener: impl PlayerEventListener,
    player_state: Arc<Mutex<PlayerState>>,
) -> Result<tokio::task::JoinHandle<()>, anyhow::Error> {
    let mut playback_notifications_stream = get_playback_notification_stream(player.clone()).await?;

    let handle = tokio::spawn(async move {
        setup_initial_player_state(player, &player_event_listener, &player_state).await.unwrap_or_default();
        info!("Player watch started");
        loop {
            let event = playback_notifications_stream.recv().await;
            match event {
                Ok(event) => {
                    process_player_event(event, &player_event_listener, &player_state).await
                }
                Err(e) => match e {
                    PlayerEventReceiveError::Closed => {
                        info!("Playback notifications stream closed");
                        break;
                    }
                    PlayerEventReceiveError::Lagged(number) => {
                        warn!(
                            "Playback notifications stream lagged {} event{}.",
                            number,
                            if number == 1 { "" } else { "s" }
                        );
                        break;
                    }
                },
            }
        }
    });
    Ok(handle)
}

async fn setup_initial_player_state(player: Player, player_event_listener: &impl PlayerEventListener, player_state: &Arc<Mutex<PlayerState>>) -> Result<(), Error> {
    let initial_state = player.get_current_state().await?;
    process_player_event(PlayerEvent::TimelineChanged(initial_state.timeline.clone()), player_event_listener, &player_state).await;
    process_player_event(PlayerEvent::StatusChanged(initial_state.status), player_event_listener, &player_state).await;
    process_player_event(PlayerEvent::TextChanged((FsctTextMetadata::CurrentTitle, initial_state.texts.title.clone())), player_event_listener, &player_state).await;
    process_player_event(PlayerEvent::TextChanged((FsctTextMetadata::CurrentAlbum, initial_state.texts.album.clone())), player_event_listener, &player_state).await;
    process_player_event(PlayerEvent::TextChanged((FsctTextMetadata::CurrentAuthor, initial_state.texts.artist.clone())), player_event_listener, &player_state).await;
    process_player_event(PlayerEvent::TextChanged((FsctTextMetadata::CurrentGenre, initial_state.texts.genre.clone())), player_event_listener, &player_state).await;
    *player_state.lock().unwrap() = initial_state;
    Ok(())
}
