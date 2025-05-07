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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use anyhow::anyhow;
use crate::player::{Player, PlayerState};
use crate::{devices_watch, player_watch};
use crate::devices_watch::DevicesPlayerEventApplier;


pub async fn run_service(player: Player) -> Result<(), anyhow::Error> {
    let fsct_devices = Arc::new(Mutex::new(HashMap::new()));
    let player_state = Arc::new(Mutex::new(PlayerState::default()));

    let player_event_listener = DevicesPlayerEventApplier::new(fsct_devices.clone());

    devices_watch::run_devices_watch(fsct_devices.clone(), player_state.clone()).await?;
    player_watch::run_player_watch(player, player_event_listener, player_state).await.map_err(|e| anyhow!(e))?;
    Ok(())
}