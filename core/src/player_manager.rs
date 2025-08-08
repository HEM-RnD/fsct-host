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
use anyhow::Error;
use log::{debug, error, info};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use nusb::DeviceId;

use crate::player_state::PlayerState;
use crate::usb::fsct_device::FsctDevice;

/// Type alias for player ID
pub type PlayerId = u32;

/// Represents a registered player with its state and device assignments
pub struct RegisteredPlayer {
    pub id: PlayerId,
    pub name: String,
    pub state: Arc<Mutex<PlayerState>>,
    pub assigned_device: Option<DeviceId>,
}

/// Manages players and their device assignments
pub struct PlayerManager {
    players: Arc<Mutex<HashMap<PlayerId, RegisteredPlayer>>>,
    devices: Arc<Mutex<HashMap<DeviceId, Arc<FsctDevice>>>>,
    next_player_id: Arc<Mutex<PlayerId>>,
}

impl PlayerManager {
    /// Creates a new PlayerManager
    pub fn new(devices: Arc<Mutex<HashMap<DeviceId, Arc<FsctDevice>>>>) -> Self {
        Self {
            players: Arc::new(Mutex::new(HashMap::new())),
            devices,
            next_player_id: Arc::new(Mutex::new(1)), // Start from 1
        }
    }

    /// Registers a new player
    pub async fn register_player(&mut self, name: String) -> Result<PlayerId, Error> {
        let player_id = {
            let mut id = self.next_player_id.lock().unwrap();
            let current_id = *id;
            *id += 1;
            current_id
        };

        let player_state = Arc::new(Mutex::new(Default::default()));

        // Create player entry
        let registered_player = RegisteredPlayer {
            id: player_id,
            name,
            state: player_state,
            assigned_device: None,
        };

        // Add to players map
        self.players.lock().unwrap().insert(player_id, registered_player);

        info!("Player {} registered", player_id);
        Ok(player_id)
    }

    /// Unregisters a player
    pub async fn unregister_player(&mut self, player_id: PlayerId) -> Result<(), Error> {
        let mut players = self.players.lock().unwrap();
        if let Some(player) = players.remove(&player_id) {
            // Unassign from all devices
            if let Some(device_id) = player.assigned_device {
                self.unassign_player_from_device_internal(player_id, device_id).await?;
            }
            info!("Player {} unregistered", player_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Player not found"))
        }
    }

    /// Assigns a player to a device
    pub async fn assign_player_to_device(&mut self, player_id: PlayerId, device_id: DeviceId) -> Result<(), Error> {
        // Check if player exists
        {
            let players = self.players.lock().unwrap();
            if !players.contains_key(&player_id) {
                return Err(anyhow::anyhow!("Player not found"));
            }
        };

        // Check if device exists
        let device = {
            let devices = self.devices.lock().unwrap();
            match devices.get(&device_id) {
                Some(device) => device.clone(),
                None => return Err(anyhow::anyhow!("Device not found")),
            }
        };

        // Set device as player's assigned device
        {
            let mut players = self.players.lock().unwrap();
            if let Some(player) = players.get_mut(&player_id) {
                player.assigned_device = Some(device_id);
            }
        }

        // Apply current player state to the device
        let player_state = {
            let players = self.players.lock().unwrap();
            players.get(&player_id).unwrap().state.lock().unwrap().clone()
        };

        self.apply_player_state_to_device(&device, &player_state).await?;

        info!("Player {} assigned to device {:?}", player_id, device_id);
        Ok(())
    }

    /// Unassigns a player from a device
    pub async fn unassign_player_from_device(&mut self, player_id: PlayerId, device_id: DeviceId) -> Result<(), Error> {
        self.unassign_player_from_device_internal(player_id, device_id).await
    }

    /// Internal implementation of unassign_player_from_device
    async fn unassign_player_from_device_internal(&self, player_id: PlayerId, device_id: DeviceId) -> Result<(), Error> {
        {
            let mut players = self.players.lock().unwrap();
            if let Some(player) = players.get_mut(&player_id) {
                if player.assigned_device == Some(device_id) {
                    player.assigned_device = None;
                } else {
                    return Err(anyhow::anyhow!("Player not assigned to device"));
                }
            } else {
                return Err(anyhow::anyhow!("Player not found"));
            }
        }

        info!("Player {} unassigned from device {:?}", player_id, device_id);
        Ok(())
    }

    /// Gets the devices assigned to a player
    pub fn get_player_assigned_devices(&self, player_id: PlayerId) -> Result<Option<DeviceId>, Error> {
        let players = self.players.lock().unwrap();
        if let Some(player) = players.get(&player_id) {
            Ok(player.assigned_device.clone())
        } else {
            Err(anyhow::anyhow!("Player not found"))
        }
    }

    /// Updates a player's state
    pub async fn update_player_state(&self, player_id: PlayerId, new_state: PlayerState) -> Result<(), Error> {
        // Update player state
        let assigned_device = {
            let players = self.players.lock().unwrap();
            if let Some(player) = players.get(&player_id) {
                // Update the state
                *player.state.lock().unwrap() = new_state.clone();
                player.assigned_device.clone()
            } else {
                return Err(anyhow::anyhow!("Player not found"));
            }
        };

        // Apply state to assigned device
        if let Some(device_id) = assigned_device {
            let device = {
                let devices = self.devices.lock().unwrap();
                match devices.get(&device_id) {
                    Some(device) => device.clone(),
                    None => return Ok(()), // Return if device not found
                }
            };

            if let Err(e) = self.apply_player_state_to_device(&device, &new_state).await {
                error!("Failed to apply state to device {:?}: {}", device_id, e);
            }
        } else {
            // todo apply state to all unassigned devices
        }

        Ok(())
    }

    /// Applies a player state to a device
    async fn apply_player_state_to_device(&self, device: &FsctDevice, state: &PlayerState) -> Result<(), Error> {
        // Apply status
        device.set_status(state.status).await?;

        // Apply timeline
        device.set_progress(state.timeline.clone()).await?;

        // Apply texts
        for (text_id, text) in state.texts.iter() {
            device.set_current_text(text_id, text.as_deref()).await?;
        }

        Ok(())
    }
}
