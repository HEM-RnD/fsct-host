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

use anyhow::Error;
use async_trait::async_trait;
use crate::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use crate::definitions::ManagedDeviceId;
use crate::definitions::ManagedPlayerId;
use crate::player_state::PlayerState;
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

    async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: Option<String>) -> Result<(), Error>;

    async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> Result<Option<ManagedDeviceId>, Error>;
}


