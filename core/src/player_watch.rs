use async_trait::async_trait;
use log::error;
use futures::channel::mpsc::{SendError, Sender};
use std::time::Duration;
use std::sync::{Arc, Mutex};
use futures::{SinkExt, StreamExt};
use crate::definitions::FsctTextMetadata;
use crate::player::{Player, PlayerError, PlayerEvent, PlayerEventsStream, PlayerInterface, PlayerState};

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
async fn update_current_status(new_state: &PlayerState, current_state: &mut PlayerState, tx: &mut
Sender<PlayerEvent>) -> Result<(), SendError> {
    if new_state.status != current_state.status {
        current_state.status = new_state.status;
        tx.send(PlayerEvent::StatusChanged(new_state.status.clone())).await?;
    }
    Ok(())
}

async fn update_timeline(new_state: &PlayerState,
                         current_state: &mut PlayerState,
                         tx: &mut Sender<PlayerEvent>) -> Result<(), SendError> {
    if new_state.timeline != current_state.timeline {
        current_state.timeline = new_state.timeline.clone();
        tx.send(PlayerEvent::TimelineChanged(new_state.timeline.clone())).await?;
    }
    Ok(())
}

async fn update_text(text_id: FsctTextMetadata,
                     new_state: &PlayerState,
                     current_state: &mut PlayerState,
                     tx: &mut Sender<PlayerEvent>) -> Result<(), SendError>
{
    let new_text = new_state.texts.get_text(text_id);
    let current_text = current_state.texts.get_mut_text(text_id);
    if new_text != current_text {
        *current_text = new_text.clone();
        tx.send(PlayerEvent::TextChanged((text_id, new_text.clone()))).await?;
    }
    Ok(())
}

async fn update_texts(new_state: &PlayerState,
                      current_state: &mut PlayerState,
                      tx: &mut Sender<PlayerEvent>) -> Result<(), SendError> {
    update_text(FsctTextMetadata::CurrentTitle, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentAuthor, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentAlbum, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentGenre, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentYear, new_state, current_state, tx).await?;

    Ok(())
}

async fn update_current_metadata(new_state: &PlayerState,
                                 current_state: &mut PlayerState,
                                 tx: &mut Sender<PlayerEvent>) -> Result<(), SendError>
{
    update_current_status(new_state, current_state, tx).await?;
    update_timeline(new_state, current_state, tx).await?;
    update_texts(new_state, current_state, tx).await?;
    Ok(())
}

fn create_polling_metadata_watch(player: Player) -> PlayerEventsStream
{
    let (mut tx, rx) = futures::channel::mpsc::channel(30);
    tokio::spawn(async move {
        let mut current_metadata = PlayerState::default();
        loop {
            let state = player.get_current_state().await.unwrap_or_default();

            if let Err(e) = update_current_metadata(&state, &mut current_metadata, &mut tx).await {
                if e.is_disconnected() {
                    break;
                }
                error!("Failed to send changes to channel: {}", e);
            }
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
                println!("Status changed to {:?}", current_state.status);
                return true;
            }
        }
        PlayerEvent::TimelineChanged(timeline) => {
            if *timeline != current_state.timeline {
                current_state.timeline = timeline.clone();
                println!("Timeline changed to {:?}", current_state.timeline);
                return true;
            }
        }
        PlayerEvent::TextChanged((text_id, text)) => {
            let current_text = current_state.texts.get_mut_text(*text_id);
            if *text != *current_text {
                *current_text = text.clone();
                println!("Text {:?} changed to {:?}", text_id, text);
                return true;
            }
        }
    };
    false
}

async fn process_player_event(event: PlayerEvent, player_event_listener: &impl PlayerEventListener,
                              current_metadata:
                              &Arc<Mutex<PlayerState>>)
{
    let has_changed = update_current_state_on_event(&event, &mut current_metadata.lock().unwrap());
    if !has_changed {
        return;
    }

    player_event_listener.on_event(event).await;
}

async fn get_playback_notification_stream(player: Player) -> Result<PlayerEventsStream, PlayerError>
{
    match player.listen_to_player_notifications().await {
        Ok(listener) => Ok(listener),
        Err(PlayerError::FeatureNotSupported) => Ok(create_polling_metadata_watch(player)),
        Err(e) => Err(e),
    }
}

pub async fn run_player_watch(player: Player,
                              player_event_listener: impl PlayerEventListener,
                              player_state: Arc<Mutex<PlayerState>>)
    -> Result<(), String>
{
    let mut playback_notifications_stream = get_playback_notification_stream(player).await.map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        while let Some(event) = playback_notifications_stream.next().await {
            process_player_event(event, &player_event_listener, &player_state).await;
        }
    });
    Ok(())
}