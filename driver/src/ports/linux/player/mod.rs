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

mod mpris;
mod player_handler;
mod player_manager;

use crate::{JoinableTaskHandle, spawn_service};
use anyhow::bail;
use fsct::FsctDriver;
use fsct::definitions::FsctStatus;
use futures_util::StreamExt;
use log::warn;
use mpris::*;
use player_manager::PlayerRegistrationManager;
use std::sync::Arc;
use tokio::select;

impl From<PlaybackStatus> for FsctStatus {
    fn from(status: PlaybackStatus) -> Self {
        match status {
            PlaybackStatus::Playing => FsctStatus::Playing,
            PlaybackStatus::Paused => FsctStatus::Paused,
            PlaybackStatus::Stopped => FsctStatus::Stopped,
        }
    }
}

pub async fn os_watcher_main_loop(
    driver: Arc<dyn FsctDriver>,
    player_watcher: mpris::SessionWatcher,
) -> anyhow::Result<()> {
    let manager = PlayerRegistrationManager::new(driver.clone());
    let stream = player_watcher.iter_player(true).await;
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

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<JoinableTaskHandle> {
    let player_watcher = mpris::SessionWatcher::new().await?;
    let handle = spawn_service(|mut stop| async move {
        select! {
            _ = stop.signaled() => {},
            _ = os_watcher_main_loop(driver.clone(), player_watcher) => {},
        }
    });
    Ok(handle)
}
