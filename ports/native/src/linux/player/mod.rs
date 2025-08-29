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
use fsct_core::{FsctDriver, ManagedPlayerId, ServiceHandle, spawn_service};
use fsct_core::player_state::PlayerState;

/// Linux OS watcher (skeleton). In future this should integrate with MPRIS over D-Bus
/// to reflect system media state. For now, it registers a placeholder player and
/// exposes a no-op background task that simply waits for shutdown.
pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    // Register a placeholder player to keep the driver interface consistent
    let _player_id: ManagedPlayerId = driver
        .register_player("native-linux-placeholder".to_string())
        .await?;

    // Optionally set initial default state (all defaults)
    let _ = driver.update_player_state(_player_id, PlayerState::default()).await;

    // Spawn a no-op service that just awaits for stop signal
    let handle = spawn_service(|mut stop| async move {
        stop.signaled().await;
    });

    Ok(handle)
}
