use crate::definitions::FsctTextMetadata;
use crate::player::{
    Player, PlayerError, PlayerEvent, PlayerEventReceiveError, PlayerEventsReceiver,
    PlayerEventsSender, PlayerInterface, PlayerState, create_player_events_channel,
};
use async_trait::async_trait;

use log::{error, info, warn};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
                info!("Status changed to {:?}", current_state.status);
                return true;
            }
        }
        PlayerEvent::TimelineChanged(timeline) => {
            if *timeline != current_state.timeline {
                current_state.timeline = timeline.clone();
                info!("Timeline changed to {:?}", current_state.timeline);
                return true;
            }
        }
        PlayerEvent::TextChanged((text_id, text)) => {
            let current_text = current_state.texts.get_mut_text(*text_id);
            if *text != *current_text {
                *current_text = text.clone();
                info!("Text {:?} changed to {:?}", text_id, text);
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
            info!("Player supports notification stream, Using it");
            Ok(listener)
        }
        Err(PlayerError::FeatureNotSupported) => {
            info!(
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
) -> Result<tokio::task::JoinHandle<()>, String> {
    let mut playback_notifications_stream = get_playback_notification_stream(player)
        .await
        .map_err(|e| e.to_string())?;
    let handle = tokio::spawn(async move {
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
