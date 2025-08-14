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

use anyhow::Error;
use async_trait::async_trait;
use tokio::sync::broadcast;
use crate::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use crate::device_manager::{DeviceManager, ManagedDeviceId};
use crate::player_events::PlayerEvent;
use crate::player_manager::{ManagedPlayerId, PlayerManager};
use crate::player_state::PlayerState;
use crate::service::MultiServiceHandle;
use crate::orchestrator::Orchestrator;
use crate::usb_device_watch::run_usb_device_watch;

/// Abstraction over FSCT host driver functionality that can be backed by a local
/// in-process implementation or a future IPC-based implementation.
#[async_trait]
pub trait FsctDriver: Send + Sync {
    // --- Player management ---
    async fn register_player(&self, self_id: String) -> Result<ManagedPlayerId, Error>;
    async fn unregister_player(&self, player_id: ManagedPlayerId) -> Result<(), Error>;

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error>;
    async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error>;

    async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> Result<(), Error>;

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> Result<(), Error>;

    async fn update_player_timeline(&self, player_id: ManagedPlayerId, new_timeline: Option<TimelineInfo>) -> Result<(), Error>;

    async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: String) -> Result<(), Error>;

    fn set_preferred_player(&self, preferred: Option<ManagedPlayerId>) -> Result<(), Error>;
    fn get_preferred_player(&self) -> Option<ManagedPlayerId>;

    fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> Result<Option<ManagedDeviceId>, Error>;

    // Events (player-facing only)
    fn subscribe_player_events(&self) -> broadcast::Receiver<PlayerEvent>;
}

/// Local, in-process implementation of FsctDriver.
/// Wraps the existing PlayerManager and DeviceManager and forwards all calls.
pub struct LocalDriver {
    player_manager: Arc<PlayerManager>,
    device_manager: Arc<DeviceManager>,
}

impl LocalDriver {
    /// Create a LocalDriver from existing managers.
    pub fn new(player_manager: Arc<PlayerManager>, device_manager: Arc<DeviceManager>) -> Self {
        Self { player_manager, device_manager }
    }

    /// Create a LocalDriver with freshly created managers.
    pub fn with_new_managers() -> Self {
        Self::new(Arc::new(PlayerManager::new()), Arc::new(DeviceManager::new()))
    }

    /// Access the underlying managers if needed by advanced callers.
    pub fn player_manager(&self) -> Arc<PlayerManager> { self.player_manager.clone() }
    pub fn device_manager(&self) -> Arc<DeviceManager> { self.device_manager.clone() }

    /// Run orchestrator and USB device watch services and return a combined handle.
    pub async fn run(&self) -> Result<MultiServiceHandle, Error> {
        // Subscribe to player events from the PlayerManager
        let player_rx = self.player_manager.subscribe();

        // Build and run the orchestrator using the DeviceManager
        let orchestrator = Orchestrator::with_device_manager(player_rx, self.device_manager.clone());
        let orch_handle = orchestrator.run();

        // Start USB device watch
        let usb_handle = run_usb_device_watch(self.device_manager.clone()).await?;

        // Combine both service handles into a MultiServiceHandle
        let mut multi = MultiServiceHandle::with_capacity(2);
        multi.add(orch_handle);
        multi.add(usb_handle);
        Ok(multi)
    }
}

#[async_trait]
impl FsctDriver for LocalDriver {
    async fn register_player(&self, self_id: String) -> Result<ManagedPlayerId, Error> {
        // register_player only needs &self
        self.player_manager.register_player(self_id).await
    }

    async fn unregister_player(&self, player_id: ManagedPlayerId) -> Result<(), Error> {
        self.player_manager.unregister_player(player_id).await
    }

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        self.player_manager.assign_player_to_device(player_id, device_id).await
    }

    async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        self.player_manager.unassign_player_from_device(player_id, device_id).await
    }

    async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> Result<(), Error> {
        self.player_manager.update_player_state(player_id, new_state).await
    }

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> Result<(), Error> {
        self.player_manager.update_player_status(player_id, new_status).await
    }

    async fn update_player_timeline(&self, player_id: ManagedPlayerId, new_timeline: Option<TimelineInfo>) -> Result<(), Error> {
        self.player_manager.update_player_timeline(player_id, new_timeline).await
    }

    async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: String) -> Result<(), Error> {
        self.player_manager.update_player_metadata(player_id, metadata_id, new_text).await
    }

    fn set_preferred_player(&self, preferred: Option<ManagedPlayerId>) -> Result<(), Error> {
        self.player_manager.set_preferred_player(preferred)
    }

    fn get_preferred_player(&self) -> Option<ManagedPlayerId> {
        self.player_manager.get_preferred_player()
    }

    fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> Result<Option<ManagedDeviceId>, Error> {
        self.player_manager.get_player_assigned_devices(player_id)
    }

    fn subscribe_player_events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.player_manager.subscribe()
    }



}
