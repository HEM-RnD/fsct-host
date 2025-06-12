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

use std::sync::{Arc, Mutex};
use anyhow::Result;
use log::{info, error, warn, debug};
use tokio::task::JoinHandle;
use crate::{run_devices_watch, run_player_watch, DevicesWatchHandle, DevicesPlayerEventApplier, player::Player};

// Struct to hold the service state and abort handles
pub struct FsctServiceState {
    pub device_watch_handle: Option<DevicesWatchHandle>,
    pub player_watch_handle: Option<JoinHandle<()>>,
}

impl FsctServiceState {
    pub fn new() -> Result<Self> {
        Ok(Self {
            device_watch_handle: None,
            player_watch_handle: None,
        })
    }

    pub async fn stop_service(&mut self) {
        info!("Stopping service tasks");
        if let Some(handle) = self.device_watch_handle.take() {
            // Request shutdown and wait for it to complete
            // This will abort the task
            match handle.shutdown().await {
                Ok(()) => {},
                Err(e) if e.is_cancelled() => {
                    // Task was cancelled, continue stopping
                    debug!("Device watch task was cancelled during shutdown");
                },
                Err(e) if e.is_panic() => {
                    // Propagate panic
                    error!("Device watch task panicked during shutdown: {}", e);
                    std::panic::resume_unwind(e.into_panic());
                },
                Err(e) => {
                    error!("Error shutting down device watch: {}", e);
                }
            }
        }

        if let Some(handle) = self.player_watch_handle.take() {
            handle.abort();
        }

        // Clear the handles
        self.player_watch_handle = None;
    }

    pub async fn start_service_with_player(&mut self, platform_player: Player) -> Result<()> {
        info!("Starting service tasks");
        if self.device_watch_handle.is_some() || self.player_watch_handle.is_some() {
            warn!("Service tasks are already running, stopping them first");
            self.stop_service().await;
        }

        // Create shared state for devices and player state
        let fsct_devices = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let player_state = Arc::new(Mutex::new(crate::player::PlayerState::default()));

        // Set up player event listener
        let player_event_listener = DevicesPlayerEventApplier::new(fsct_devices.clone());

        // Start devices watch
        debug!("Starting devices watch");
        let device_watch_handle = run_devices_watch(fsct_devices.clone(), player_state.clone()).await?;
        self.device_watch_handle = Some(device_watch_handle);

        // Start player watch
        debug!("Starting player watch");
        let player_watch_handle = run_player_watch(platform_player, player_event_listener, player_state).await?;
        self.player_watch_handle = Some(player_watch_handle);

        info!("Service tasks started successfully");
        Ok(())
    }

    pub fn abort(mut self) {
        self.device_watch_handle.take().unwrap().abort();
        self.player_watch_handle.take().unwrap().abort();
    }
}
