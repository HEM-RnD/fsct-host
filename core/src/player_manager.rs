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
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use anyhow::Error;
use log::{info};

use crate::device_manager::ManagedDeviceId;
use crate::player_events::PlayerEvent;
use crate::player_state::PlayerState;
use tokio::sync::broadcast;

/// Type alias for player ID
pub type ManagedPlayerId = NonZeroU32;

#[allow(dead_code)]
/// Represents a registered player with its state and device assignments
pub struct RegisteredPlayer {
    pub self_id: String, /// Player's self identifier
    pub state: Arc<Mutex<PlayerState>>,
    pub assigned_device: Option<ManagedDeviceId>,
}

/// Manages players and their device assignments
pub struct PlayerManager {
    players: Arc<Mutex<HashMap<ManagedPlayerId, RegisteredPlayer>>>,
    events_tx: broadcast::Sender<PlayerEvent>,
    next_player_id: AtomicU32,
    preferred_player_id: AtomicU32, // 0 = None, NonZeroU32 = Some
}

impl PlayerManager {
    /// Creates a new PlayerManager
    pub fn new() -> Self {
        let (events_tx, _) = broadcast::channel(256);
        Self {
            players: Arc::new(Mutex::new(HashMap::new())),
            events_tx,
            next_player_id: AtomicU32::new(1), // Start from 1
            preferred_player_id: AtomicU32::new(0), // None by default
        }
    }

    /// Subscribes to player events emitted by this manager.
    pub fn subscribe(&self) -> broadcast::Receiver<PlayerEvent> {
        self.events_tx.subscribe()
    }

    /// Registers a new player
    pub async fn register_player(&self, self_id: String) -> Result<ManagedPlayerId, Error> {
        let player_id = self.assign_new_player_id();

        let player_state = Arc::new(Mutex::new(Default::default()));

        // Create player entry
        let registered_player = RegisteredPlayer {
            self_id: self_id.clone(),
            state: player_state,
            assigned_device: None,
        };

        // Add to players map
        self.players.lock().unwrap().insert(player_id, registered_player);

        // Notify listeners
        let _ = self.events_tx.send(PlayerEvent::Registered { player_id, self_id });

        info!("Player {} registered", player_id);
        Ok(player_id)
    }
    fn assign_new_player_id(&self) -> ManagedPlayerId {
        let id_u32 = self.next_player_id.fetch_add(1, Ordering::SeqCst);
        // Safety: next_player_id starts at 1 and only increments
        NonZeroU32::new(id_u32).expect("ManagedPlayerId must be non-zero")
    }

    /// Unregisters a player
    pub async fn unregister_player(&self, player_id: ManagedPlayerId) -> Result<(), Error> {
        // Remove the player and capture assigned device without holding the lock across await
        let assigned_device = {
            let mut players = self.players.lock().unwrap();
            if let Some(player) = players.remove(&player_id) {
                player.assigned_device
            } else {
                return Err(anyhow::anyhow!("Player not found"));
            }
        };

        // Unassign from device if assigned (no players lock held here)
        if let Some(device_id) = assigned_device {
            let _ = self.events_tx.send(PlayerEvent::Unassigned { player_id, device_id });
            info!("Player {} unassigned from device {}", player_id, device_id);
        }

        // If this player was preferred, clear preference and notify
        let current_pref = self.preferred_player_id.load(Ordering::SeqCst);
        if current_pref == player_id.get() {
            let _ = self.preferred_player_id.compare_exchange(player_id.get(), 0, Ordering::SeqCst, Ordering::SeqCst);
            let _ = self.events_tx.send(PlayerEvent::PreferredChanged { preferred: None });
        }
        // Notify listeners
        let _ = self.events_tx.send(PlayerEvent::Unregistered { player_id });

        info!("Player {} unregistered", player_id);
        Ok(())
    }

    /// Assigns a player to a device
    pub async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        let player_state = {
            let mut players = self.players.lock().unwrap();
            if let Some(player) = players.get_mut(&player_id) {
                player.assigned_device = Some(device_id);
                player.state.lock().unwrap().clone()
            } else {
                return Err(anyhow::anyhow!("Player not found"));
            }
        };

        // Notify about assignment
        let _ = self.events_tx.send(PlayerEvent::Assigned { player_id, device_id });
        // Also emit current state so consumers may immediately propagate it if needed
        let _ = self.events_tx.send(PlayerEvent::StateUpdated { player_id, state: player_state });

        info!("Player {} assigned to device {}", player_id, device_id);
        Ok(())
    }

    /// Unassigns a player from a device
    pub async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        self.unassign_player_from_device_internal(player_id, device_id).await
    }

    /// Internal implementation of unassign_player_from_device
    async fn unassign_player_from_device_internal(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        {
            let mut players = self.players.lock().unwrap();
            if let Some(player) = players.get_mut(&player_id) {
                if player.assigned_device == Some(device_id) {
                    player.assigned_device = None;
                } else {
                    return Err(anyhow::anyhow!("Player not assigned to the device"));
                }
            } else {
                return Err(anyhow::anyhow!("Player not found"));
            }
        }

        // Notify listeners about unassignment
        let _ = self.events_tx.send(PlayerEvent::Unassigned { player_id, device_id });

        info!("Player {} unassigned from device {}", player_id, device_id);
        Ok(())
    }

    /// Gets the devices assigned to a player
    pub fn get_player_assigned_devices(&self, player_id: ManagedPlayerId) -> Result<Option<ManagedDeviceId>, Error> {
        let players = self.players.lock().unwrap();
        if let Some(player) = players.get(&player_id) {
            Ok(player.assigned_device)
        } else {
            Err(anyhow::anyhow!("Player not found"))
        }
    }

    /// Updates a player's state
    pub async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> Result<(), Error> {
        {
            let players = self.players.lock().unwrap();
            if let Some(player) = players.get(&player_id) {
                *player.state.lock().unwrap() = new_state.clone();
            } else {
                return Err(anyhow::anyhow!("Player not found"));
            }
        }

        // Notify listeners about the new state
        let _ = self.events_tx.send(PlayerEvent::StateUpdated { player_id, state: new_state });

        Ok(())
    }

    /// Sets the preferred player to Some(id) or clears it with None.
    /// Emits a single PreferredChanged event if the value changed.
    pub fn set_preferred_player(&self, preferred: Option<ManagedPlayerId>) -> Result<(), Error> {
        // Validate existence if Some
        if let Some(pid) = preferred {
            let players = self.players.lock().unwrap();
            if !players.contains_key(&pid) {
                return Err(anyhow::anyhow!("Player not found"));
            }
        }
        let new_val = preferred.map(ManagedPlayerId::get).unwrap_or(0);
        let old_val = self.preferred_player_id.swap(new_val, Ordering::SeqCst);
        if old_val != new_val {
            let _ = self.events_tx.send(PlayerEvent::PreferredChanged { preferred });
        }
        Ok(())
    }

    /// Returns the currently preferred player, if any.
    pub fn get_preferred_player(&self) -> Option<ManagedPlayerId> {
        NonZeroU32::new(self.preferred_player_id.load(Ordering::SeqCst))
    }
}
