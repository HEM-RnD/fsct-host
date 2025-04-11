#![deny(clippy::all)]

mod js_types;

#[macro_use]
extern crate napi_derive;

use async_trait::async_trait;
use fsct_core::definitions::{FsctStatus, FsctTextMetadata};
use fsct_core::player::{
    create_player_events_channel, PlayerError, PlayerEvent, PlayerEventsReceiver,
    PlayerEventsSender,
};
use fsct_core::{player::Player, player::PlayerInterface, player::PlayerState, run_service};
use std::sync::{Arc, Mutex};

use js_types::{CurrentTextMetadata, FsctTimelineInfo, PlayerStatus, TimelineInfo};

pub struct NodePlayerImpl {
    current_state: Mutex<PlayerState>,
    player_sender: PlayerEventsSender,
}

impl NodePlayerImpl {
    fn new() -> Self {
        let (tx, _rx) = create_player_events_channel();
        Self {
            current_state: Mutex::new(PlayerState::default()),
            player_sender: tx,
        }
    }

    fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        let status: FsctStatus = status.into();
        self.current_state.lock().unwrap().status = status;

        self.emit(PlayerEvent::StatusChanged(status))
    }

    fn set_timeline(&self, timeline: Option<&TimelineInfo>) -> napi::Result<()> {
        let timeline: Option<FsctTimelineInfo> = timeline.map(|v| (*v).into()).to_owned();
        self.current_state.lock().unwrap().timeline = timeline.clone();

        self.emit(PlayerEvent::TimelineChanged(timeline))
    }

    fn set_text(&self, text_type: CurrentTextMetadata, text: Option<String>) -> napi::Result<()> {
        let text_type: FsctTextMetadata = text_type.into();
        *self
            .current_state
            .lock()
            .unwrap()
            .texts
            .get_mut_text(text_type) = text.clone();

        self.emit(PlayerEvent::TextChanged((text_type, text.clone())))
    }

    fn emit(&self, event: PlayerEvent) -> napi::Result<()> {
        self.player_sender
            .send(event)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl PlayerInterface for NodePlayerImpl {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        Ok(self.current_state.lock().unwrap().clone())
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        Ok(self.player_sender.subscribe())
    }
}

#[napi]
pub struct NodePlayer {
    player_impl: Arc<NodePlayerImpl>,
}

#[napi]
impl NodePlayer {
    #[napi(constructor)]
    pub fn new() -> Self {
        NodePlayer {
            player_impl: Arc::new(NodePlayerImpl::new()),
        }
    }

    #[napi]
    pub fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        self.player_impl.set_status(status)
    }

    #[napi]
    pub fn set_timeline(&self, timeline: Option<&TimelineInfo>) -> napi::Result<()> {
        self.player_impl.set_timeline(timeline)
    }

    #[napi]
    pub fn set_text(
        &self,
        text_type: CurrentTextMetadata,
        text: Option<String>,
    ) -> napi::Result<()> {
        self.player_impl.set_text(text_type, text)
    }
}

#[napi]
async fn run_fsct(player: &NodePlayer) -> napi::Result<()> {
    run_service(Player::from_arc(player.player_impl.clone()))
        .await
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}
