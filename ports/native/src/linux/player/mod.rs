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
use futures_util::StreamExt;
use log::{info, warn};
use fsct_core::{FsctDriver, ManagedPlayerId, ServiceHandle, spawn_service};
use crate::linux::player::mpris::Player;

mod mpris;

async fn register_player(driver: &dyn FsctDriver, player: Player) -> Result<(), anyhow::Error> {
    let id = driver.register_player(player.identity.clone()).await?;
    info!("Registered player: {}", id);
    Ok(())
}

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    let player_watcher = mpris::PlayerWatcher::new().await?;
    let handle = spawn_service(|_stop_| async move {
        let mut stream = player_watcher.iter_player(true).await;
        futures_util::pin_mut!(stream);
        while let Some(player) = stream.next().await {
            match player {
                Ok(player) => {
                    let player_registration_result = register_player(driver.as_ref(), player).await;
                    if let Err(e) = player_registration_result {
                        warn!("Error registering player: {}", e);
                    }
                }
                Err(e) => warn!("Error connecting to player: {}", e),
            }
        }
    });
    Ok(handle)
}
