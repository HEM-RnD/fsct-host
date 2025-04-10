#![deny(clippy::all)]

mod js_types;

#[macro_use]
extern crate napi_derive;

use fsct_core::definitions::{FsctStatus, FsctTextMetadata};
use fsct_core::{player::PlayerState, player::Player, player::PlayerInterface, run_service};
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use fsct_core::player::PlayerError;

use js_types::{PlayerStatus, CurrentTextMetadata, TimelineInfo, FsctTimelineInfo};

pub struct NodePlayerImpl {
    current_state: Mutex<PlayerState>,
}

impl NodePlayerImpl {
    pub fn new() -> Self {
        Self { current_state: Mutex::new(PlayerState::default()) }
    }

    pub async fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        let status: FsctStatus = status.into();
        self.current_state.lock().unwrap().status = status;

        // here emit event

        Ok(())
    }

    pub async fn set_timeline(&self,
                              timeline: Option<&TimelineInfo>) -> napi::Result<()> {
        let timeline: Option<FsctTimelineInfo> = timeline.map(|v|(*v).into()).to_owned();
        self.current_state.lock().unwrap().timeline = timeline;

        // here emit event

        Ok(())
    }

    pub async fn set_text(&self, text_type: CurrentTextMetadata, text: Option<String>) -> napi::Result<()> {
        let text_type: FsctTextMetadata = text_type.into();
        *self.current_state.lock().unwrap().texts.get_mut_text(text_type) = text;

        // here emit event

        Ok(())
    }
}

#[async_trait]
impl PlayerInterface for NodePlayerImpl {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        Ok(self.current_state.lock().unwrap().clone())
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
        NodePlayer { player_impl: Arc::new(NodePlayerImpl::new()) }
    }

    #[napi]
    pub async fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        self.player_impl.set_status(status).await
    }

    #[napi]
    pub async fn set_timeline(&self,
                              timeline: Option<&TimelineInfo>) -> napi::Result<()> {
        self.player_impl.set_timeline(timeline).await
    }

    #[napi]
    pub async fn set_text(&self, text_type: CurrentTextMetadata, text: Option<String>) -> napi::Result<()> {
        self.player_impl.set_text(text_type, text).await
    }
}


#[napi]
async fn run_fsct(player: &NodePlayer) -> napi::Result<()> {
    run_service(Player::from_arc(player.player_impl.clone())).await.map_err(|e| napi::Error::from_reason(e.to_string()))
}
