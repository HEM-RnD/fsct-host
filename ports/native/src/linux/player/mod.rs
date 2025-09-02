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

use std::fmt::format;
use std::sync::Arc;
use anyhow::bail;
use futures_util::future::select;
use futures_util::StreamExt;
use log::{info, warn};
use tokio::select;
use tokio::task::JoinSet;
use fsct_core::{spawn_service, FsctDriver, ManagedPlayerId, ServiceHandle};
use crate::linux::player::mpris::SessionWatcher;
use tokio_util::sync::{CancellationToken, DropGuard};
use mpris::Player;

mod mpris;


struct PlayerRegistrationManager {
    driver: Arc<dyn FsctDriver>,
    cancellation_token: CancellationToken,
    drop_guard: DropGuard,
}

impl PlayerRegistrationManager {
    fn new(driver: Arc<dyn FsctDriver>) -> Self {
        let cancellation_token = CancellationToken::new();
        let drop_guard = cancellation_token.clone().drop_guard();
        Self { driver, cancellation_token, drop_guard }
    }

    async fn register_player(&self, player: Player) -> Result<(), anyhow::Error> {
        let id = self.driver.register_player(player.name()).await?;
        let cancel_token = self.cancellation_token.child_token();
        info!("Registered player: {}", id);
        let driver = self.driver.clone();
        tokio::spawn(async move {
            let player_handler = PlayerHandler::new(player, id);
            select! {
                _ = cancel_token.cancelled() => {},
                _ = player_handler.handle_player_task() => {}
            }
            let _ = driver.unregister_player(id).await;
            info!("Unregistered player: {}", id);
        });
        Ok(())
    }
}

struct PlayerHandler {
    player: Player,
    id: ManagedPlayerId,
}

impl PlayerHandler {
    fn new(player: Player, id: ManagedPlayerId) -> Self {
        Self {
            player,
            id,
        }
    }

    async fn handle_player_task(&self) -> anyhow::Result<()> {
        let player_proxy = self.player.as_player_interface().await?;
        let seeked_signal = player_proxy.receive_seeked().await?;
        let playback_status_changed_signal = player_proxy.receive_playback_status_changed().await;
        let metadata_changed_signal = player_proxy.receive_metadata_changed().await;
        select! {
                res = self.player.wait_for_disconnect() => {
                    res?;
                }
            }
        Ok(())
    }
}

pub async fn run_os_player_watcher(driver: Arc<dyn FsctDriver>, player_watcher: SessionWatcher) -> anyhow::Result<()> {
    let manager = PlayerRegistrationManager::new(driver.clone());
    let mut stream = player_watcher.iter_player(true).await;
    futures_util::pin_mut!(stream);
    while let Some(player) = stream.next().await {
        match player {
            Ok(player) => {
                let player_registration_result = manager.register_player(player).await;
                if let Err(e) = player_registration_result {
                    warn!("Error registering player: {}", e);
                }
            }
            Err(e) => warn!("Error connecting to player: {}", e),
        }
    }
    bail!("Player watcher stopped unexpectedly");
}

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    let player_watcher = mpris::SessionWatcher::new().await?;
    let handle = spawn_service(|mut stop| async move {
        select! {
            _ = stop.signaled() => {},
            _ = run_os_player_watcher(driver.clone(), player_watcher) => {},
        }
    });
    Ok(handle)
}
