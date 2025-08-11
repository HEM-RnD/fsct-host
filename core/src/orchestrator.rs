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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Error;
use log::{debug, info, warn};
use tokio::select;
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;

use crate::device_manager::{DeviceControl, DeviceEvent, DeviceManager, ManagedDeviceId};
use crate::player_events::PlayerEvent;
use crate::player_manager::ManagedPlayerId;
use crate::player_state::PlayerState;
use crate::player_state_applier::{DirectDeviceControlApplier, PlayerStateApplier};

/// Handle to control the orchestrator task
pub struct OrchestratorHandle {
    join: JoinHandle<()>,
    shutdown_tx: oneshot::Sender<()>,
}

impl OrchestratorHandle {
    pub async fn shutdown(self) -> Result<(), tokio::task::JoinError> {
        let _ = self.shutdown_tx.send(());
        self.join.await
    }

    pub fn abort(self) {
        self.join.abort();
    }
}

/// Orchestrator subscribes to PlayerManager and DeviceManager events
/// and applies routing policy to update devices using a PlayerStateApplier.
pub struct Orchestrator {
    // Receivers
    player_rx: broadcast::Receiver<PlayerEvent>,
    device_rx: broadcast::Receiver<DeviceEvent>,

    // Applier that performs device I/O
    applier: Arc<dyn PlayerStateApplier>,

    // Routing state
    player_to_device: HashMap<ManagedPlayerId, ManagedDeviceId>,
    device_to_player: HashMap<ManagedDeviceId, ManagedPlayerId>,
    connected_devices: HashSet<ManagedDeviceId>,
    last_state_per_player: HashMap<ManagedPlayerId, PlayerState>,
    active_unassigned: Option<ManagedPlayerId>, // policy: last updated among unassigned
}

impl Orchestrator {
    /// Create orchestrator using a DeviceManager directly (DirectDeviceControlApplier).
    pub fn with_device_manager(
        player_rx: broadcast::Receiver<PlayerEvent>,
        device_manager: Arc<DeviceManager>,
    ) -> Self {
        let applier = Arc::new(DirectDeviceControlApplier::new(device_manager.clone()));
        let device_rx = device_manager.subscribe();
        Self::new_with_applier(player_rx, device_rx, applier)
    }

    /// Create orchestrator with a custom PlayerStateApplier and a device events receiver.
    pub fn new_with_applier(
        player_rx: broadcast::Receiver<PlayerEvent>,
        device_rx: broadcast::Receiver<DeviceEvent>,
        applier: Arc<dyn PlayerStateApplier>,
    ) -> Self {
        Self {
            player_rx,
            device_rx,
            applier,
            player_to_device: HashMap::new(),
            device_to_player: HashMap::new(),
            connected_devices: HashSet::new(),
            last_state_per_player: HashMap::new(),
            active_unassigned: None,
        }
    }

    /// Spawn the orchestrator event loop in background and return a handle.
    pub fn run(mut self) -> OrchestratorHandle {
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            loop {
                select! {
                    biased;
                    _ = &mut shutdown_rx => {
                        info!("Orchestrator shutdown requested");
                        break;
                    }
                    recv_res = self.player_rx.recv() => {
                        match recv_res {
                            Ok(evt) => self.on_player_event(evt).await,
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("PlayerEvent lagged by {} messages; catching up", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("PlayerEvent channel closed; stopping orchestrator");
                                break;
                            }
                        }
                    }
                    recv_res = self.device_rx.recv() => {
                        match recv_res {
                            Ok(evt) => self.on_device_event(evt).await,
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("DeviceEvent lagged by {} messages; catching up", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("DeviceEvent channel closed; stopping orchestrator");
                                break;
                            }
                        }
                    }
                }
            }
        });
        OrchestratorHandle { join, shutdown_tx }
    }

    async fn on_player_event(&mut self, evt: PlayerEvent) {
        match evt {
            PlayerEvent::Registered { player_id, .. } => {
                debug!("Player registered: {}", player_id);
            }
            PlayerEvent::Unregistered { player_id } => {
                debug!("Player unregistered: {}", player_id);
                self.last_state_per_player.remove(&player_id);
                if let Some(dev) = self.player_to_device.remove(&player_id) {
                    self.device_to_player.remove(&dev);
                    // Device becomes unassigned; if we have an active unassigned, update it
                    if let Some(active) = self.active_unassigned {
                        if let Some(state) = self.last_state_per_player.get(&active) {
                            if self.connected_devices.contains(&dev) {
                                let _ = self.applier.apply_to_device(dev, state).await;
                            }
                        }
                    }
                }
                if self.active_unassigned == Some(player_id) {
                    self.active_unassigned = self.pick_active_unassigned();
                    self.propagate_unassigned().await;
                }
            }
            PlayerEvent::Assigned { player_id, device_id } => {
                debug!("Assigned: player {} -> device {}", player_id, device_id);
                self.player_to_device.insert(player_id, device_id);
                self.device_to_player.insert(device_id, player_id);
                if let Some(state) = self.last_state_per_player.get(&player_id) {
                    if self.connected_devices.contains(&device_id) {
                        let _ = self.applier.apply_to_device(device_id, state).await;
                    }
                }
                // If this player was the active unassigned, recompute and propagate
                if self.active_unassigned == Some(player_id) {
                    self.active_unassigned = self.pick_active_unassigned();
                    self.propagate_unassigned().await;
                }
            }
            PlayerEvent::Unassigned { player_id, device_id } => {
                debug!("Unassigned: player {} -/-> device {}", player_id, device_id);
                self.player_to_device.remove(&player_id);
                self.device_to_player.remove(&device_id);
                // Device becomes unassigned: if we have an active unassigned, update it
                if let Some(active) = self.active_unassigned {
                    if let Some(state) = self.last_state_per_player.get(&active) {
                        if self.connected_devices.contains(&device_id) {
                            let _ = self.applier.apply_to_device(device_id, state).await;
                        }
                    }
                }
            }
            PlayerEvent::StateUpdated { player_id, state } => {
                debug!("StateUpdated: player {}", player_id);
                self.last_state_per_player.insert(player_id, state.clone());
                if let Some(device_id) = self.player_to_device.get(&player_id).copied() {
                    if self.connected_devices.contains(&device_id) {
                        let _ = self.applier.apply_to_device(device_id, &state).await;
                    }
                } else {
                    // Unassigned: update policy and propagate to all unassigned devices
                    self.active_unassigned = Some(player_id);
                    self.propagate_unassigned().await;
                }
            }
        }
    }

    async fn on_device_event(&mut self, evt: DeviceEvent) {
        match evt {
            DeviceEvent::Added(device_id) => {
                debug!("Device added: {}", device_id);
                self.connected_devices.insert(device_id);
                // If device has assigned player -> apply its state; otherwise apply active unassigned
                if let Some(player_id) = self.device_to_player.get(&device_id).copied() {
                    if let Some(state) = self.last_state_per_player.get(&player_id) {
                        let _ = self.applier.apply_to_device(device_id, state).await;
                    }
                } else if let Some(active) = self.active_unassigned {
                    if let Some(state) = self.last_state_per_player.get(&active) {
                        let _ = self.applier.apply_to_device(device_id, state).await;
                    }
                }
            }
            DeviceEvent::Removed(device_id) => {
                debug!("Device removed: {}", device_id);
                self.connected_devices.remove(&device_id);
                if let Some(player_id) = self.device_to_player.remove(&device_id) {
                    self.player_to_device.remove(&player_id);
                }
            }
        }
    }

    fn pick_active_unassigned(&self) -> Option<ManagedPlayerId> {
        // Minimal policy: keep current value if still unassigned, otherwise None.
        // Could be extended to pick by last update timestamp if tracked.
        self.active_unassigned
            .filter(|pid| !self.player_to_device.contains_key(pid))
    }

    async fn propagate_unassigned(&self) {
        let Some(active) = self.active_unassigned else { return; };
        let Some(state) = self.last_state_per_player.get(&active) else { return; };
        for dev in self.connected_devices.iter() {
            if !self.device_to_player.contains_key(dev) {
                let _ = self.applier.apply_to_device(*dev, state).await;
            }
        }
    }
}
