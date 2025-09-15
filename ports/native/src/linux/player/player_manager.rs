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

use std::sync::Arc;
use log::{debug, error};
use fsct_core::FsctDriver;
use tokio_util::sync::{CancellationToken, DropGuard};
use tokio::select;
use crate::linux::player::mpris;
use crate::linux::player::player_handler::PlayerHandler;

pub struct PlayerRegistrationManager {
    driver: Arc<dyn FsctDriver>,
    cancellation_token: CancellationToken,
    _drop_guard: DropGuard,
}

impl PlayerRegistrationManager {
    pub fn new(driver: Arc<dyn FsctDriver>) -> Self {
        let cancellation_token = CancellationToken::new();
        let drop_guard = cancellation_token.clone().drop_guard();
        Self { driver, cancellation_token, _drop_guard: drop_guard }
    }

    pub async fn register_player(&self, player: mpris::Player) -> Result<(), anyhow::Error> {
        let id = self.driver.register_player(player.name()).await?;
        self.run_player_handler(player, id);
        Ok(())
    }

    fn run_player_handler(&self, player: mpris::Player, id: fsct_core::ManagedPlayerId) -> ()
    {
        let cancel_token = self.cancellation_token.child_token();
        let driver = self.driver.clone();
        tokio::spawn(async move {
            let player_handler = PlayerHandler::new(player, id, driver.clone());
            select! {
                _ = cancel_token.cancelled() => {
                    debug!("Player {} handler cancelled by parent", id);
                },
                res = player_handler.handle_player_task() => {
                    debug!("Player {} was disconnected", id);
                    if let Err(e) = res {
                        error!("Error handling player {}: {}", id, e);
                    }
                },
            }
            let _ = driver.unregister_player(id).await;
        });
    }
}