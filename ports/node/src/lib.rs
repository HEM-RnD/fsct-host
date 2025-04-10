#![deny(clippy::all)]

mod js_types;

#[macro_use]
extern crate napi_derive;

use fsct_core::definitions::FsctStatus;
use fsct_core::{player::PlayerState, player::Player, player::PlayerInterface, run_service};
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use fsct_core::player::PlayerError;

use js_types::PlayerStatus;
use js_types::get_timeline_info;

pub struct NodePlayer {
    current_state: Mutex<PlayerState>,
}

impl NodePlayer {
    pub fn new() -> Self {
        Self { current_state: Mutex::new(PlayerState::default()) }
    }
}

#[async_trait]
impl PlayerInterface for NodePlayer {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        Ok(self.current_state.lock().unwrap().clone())
    }

}

#[napi(js_name = "NodePlayer")]
pub struct JsNodePlayer {
    player_impl: Arc<NodePlayer>,
}



#[napi]
impl JsNodePlayer {
    #[napi(constructor)]
    pub fn new() -> Self {
        JsNodePlayer { player_impl: Arc::new(NodePlayer::new()) }
    }

    #[napi]
    pub async fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        let mut current_state = self.player_impl.current_state.lock().unwrap();
        let status: FsctStatus = status.into();
        if current_state.status != status {
            current_state.status = status;
        }
        Ok(())
    }

    #[napi]
    pub async fn set_timeline(&self,
                              position: f64,
                              duration: f64,
                              rate: f64) -> napi::Result<()> {
        let mut current_state = self.player_impl.current_state.lock().unwrap();
        let timeline_info = get_timeline_info(position, duration, rate);
        if current_state.timeline != timeline_info {
            current_state.timeline = timeline_info;
        }
        Ok(())
    }
}


#[napi]
async fn run_fsct(player: &JsNodePlayer) -> napi::Result<()> {
    run_service(Player::from_arc(player.player_impl.clone())).await.map_err(|e| napi::Error::from_reason(e.to_string()))
}
