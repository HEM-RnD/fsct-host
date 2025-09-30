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

#[cfg(test)]
mod tests;
mod scoring;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use log::{debug, info, warn};
use tokio::select;
use tokio::sync::broadcast;
use scoring::{Assignment, PlayerSelectionParams};
use crate::definitions::{FsctStatus, FsctTextMetadata, ManagedDeviceId, TimelineInfo};
use crate::device_manager::{DeviceEvent, DeviceManager};
use crate::device_manager::DeviceControl;
use crate::player_events::PlayerEvent;
use crate::definitions::ManagedPlayerId;
use crate::player_state::PlayerState;
use crate::player_state_applier::{DirectDeviceControlApplier, PlayerStateApplier};
use crate::joinable_task::{spawn_service, JoinableTaskHandle};

#[derive(Debug, Clone, Default)]
struct RegisteredPlayer {
    assigned_device: Option<ManagedDeviceId>,
    state: PlayerState,
}

impl RegisteredPlayer {
    fn has_metadata(&self) -> bool {
        self.state.texts.iter().any(|(_, text)| text.as_ref().map(|t| !t.is_empty()).unwrap_or(false))
    }


    fn score_for_player_and_device(&self, device_id: &ManagedDeviceId, is_last_selected: bool) -> isize {
        let assignment_state = match self.assigned_device.as_ref() {
            Some(id) if *id == *device_id => Assignment::AssignedToThisDevice,
            Some(_) => Assignment::AssignedToOtherDevice,
            None => Assignment::Unassigned,
        };

        let player_selection_params = PlayerSelectionParams {
            status: self.state.status.into(),
            is_last_selected,
            assignment: assignment_state,
            has_metadata: self.has_metadata(),
        };
        player_selection_params.score()
    }
}

#[derive(Debug, Clone, Default)]
struct ConnectedDevice {
    player_id: Option<ManagedPlayerId>,
    requires_update: bool,
}


/// Orchestrator subscribes to PlayerManager and DeviceManager events
/// and applies routing policy to update devices using a PlayerStateApplier.
pub struct Orchestrator<A: PlayerStateApplier> {
    // Receivers
    player_rx: broadcast::Receiver<PlayerEvent>,
    device_rx: broadcast::Receiver<DeviceEvent>,

    // Applier that performs device I/O
    applier: Arc<A>,

    // Routing state
    players: HashMap<ManagedPlayerId, RegisteredPlayer>,

    connected_devices: HashMap<ManagedDeviceId, Mutex<ConnectedDevice>>,
}

impl<A: PlayerStateApplier + 'static> Orchestrator<A> {
    /// Create orchestrator with a custom PlayerStateApplier and a device events receiver.
    pub fn new_with_applier(
        player_rx: broadcast::Receiver<PlayerEvent>,
        device_rx: broadcast::Receiver<DeviceEvent>,
        applier: Arc<A>,
    ) -> Self {
        Self {
            player_rx,
            device_rx,
            applier,
            players: HashMap::new(),
            connected_devices: HashMap::new(),
        }
    }
}

impl Orchestrator<DirectDeviceControlApplier<DeviceManager>> {
    /// Create orchestrator using a DeviceManager directly (DirectDeviceControlApplier).
    pub fn with_device_manager(
        player_rx: broadcast::Receiver<PlayerEvent>,
        device_manager: Arc<DeviceManager>,
    ) -> Self {
        let applier = Arc::new(DirectDeviceControlApplier::new(device_manager.clone()));
        let device_rx = device_manager.subscribe();
        Self::new_with_applier(player_rx, device_rx, applier)
    }
}

impl<A: PlayerStateApplier + 'static> Orchestrator<A> {
    /// Spawn the orchestrator event loop in background and return a handle.
    pub fn run(mut self) -> JoinableTaskHandle {
        spawn_service(move |mut stop_handle| async move {
            loop {
                select! {
                    biased;
                    _ = stop_handle.signaled() => {
                        info!("Orchestrator shutdown requested");
                        break;
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
                }
            }
        })
    }

    async fn on_player_event(&mut self, evt: PlayerEvent) {
        match evt {
            PlayerEvent::Registered { player_id, .. } => {
                self.handle_player_registered(player_id).await;
            }
            PlayerEvent::Unregistered { player_id } => {
                self.handle_player_unregistered(player_id).await;
            }
            PlayerEvent::Assigned { player_id, device_id } => {
                self.handle_player_assigned(player_id, device_id).await;
            }
            PlayerEvent::Unassigned { player_id, device_id } => {
                self.handle_player_unassigned(player_id, device_id).await;
            }
            PlayerEvent::StateUpdated { player_id, state } => {
                self.handle_player_state_updated(player_id, state).await;
            }
            PlayerEvent::StatusUpdated { player_id, status } => {
                self.handle_player_status_updated(player_id, status).await;
            }
            PlayerEvent::TimelineUpdated { player_id, timeline } => {
                self.handle_player_timeline_updated(player_id, timeline).await;
            }
            PlayerEvent::TextMetadataUpdated { player_id, metadata, text } => {
                self.handle_player_text_metadata_updated(player_id, metadata, text).await;
            }
        }
    }

    async fn on_device_event(&mut self, evt: DeviceEvent) {
        match evt {
            DeviceEvent::Added(device_id) => {
                self.handle_device_added(device_id).await;
            }
            DeviceEvent::Removed(device_id) => {
                self.handle_device_removed(device_id).await;
            }
        }
    }

    // Dedicated handlers for PlayerEvent variants
    async fn handle_player_registered(&mut self, player_id: ManagedPlayerId) {
        debug!("Player registered: {}", player_id);
        self.players.insert(player_id, RegisteredPlayer::default());
        // do nothing, because it is in idle state, so there is nothing to show, no assigment etc.
    }

    async fn handle_player_unregistered(&mut self, player_id: ManagedPlayerId) {
        debug!("Player unregistered: {}", player_id);
        self.players.remove(&player_id);

        self.update_selected_players_for_devices();
        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_player_assigned(&mut self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) {
        debug!("Assigned: player {} -> device {}", player_id, device_id);
        if let Some(player) = self.players.get_mut(&player_id) {
            player.assigned_device = Some(device_id);
        }

        self.update_selected_players_for_devices();
        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_player_unassigned(&mut self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) {
        debug!("Unassigned: player {} -/-> device {}", player_id, device_id);

        if let Some(player) = self.players.get_mut(&player_id) {
            player.assigned_device = None;
        }

        self.update_selected_players_for_devices();

        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_player_state_updated(&mut self, player_id: ManagedPlayerId, state: PlayerState) {
        debug!("StateUpdated: player {}", player_id);

        let mut status_changed = false;
        let mut metadata_changed = false;

        if let Some(player) = self.players.get_mut(&player_id) {
            let had_metadata = player.has_metadata();
            if player.state.status != state.status {
                status_changed = true;
            }
            player.state = state;
            let has_metadata_now = player.has_metadata();
            metadata_changed = had_metadata != has_metadata_now;
        }

        if status_changed || metadata_changed {
            self.update_selected_players_for_devices();
        }
        self.mark_devices_require_update_by_player(player_id);
        self.apply_on_devices_requiring_update().await;
    }

    fn mark_devices_require_update_by_player(&mut self, player_id: ManagedPlayerId) {
        for device in self.connected_devices.values() {
            let mut device = device.lock().unwrap();
            if device.player_id == Some(player_id) {
                device.requires_update = true;
            }
        }
    }

    async fn handle_player_status_updated(&mut self, player_id: ManagedPlayerId, status: FsctStatus) {
        debug!("StatusUpdated: player {} -> {:?}", player_id, status);
        if let Some(player) = self.players.get_mut(&player_id) {
            player.state.status = status;
        }
        // Status change can affect selection
        self.update_selected_players_for_devices();
        // Mark devices currently showing this player for update
        self.mark_devices_require_update_by_player(player_id);
        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_player_timeline_updated(&mut self, player_id: ManagedPlayerId, timeline: TimelineInfo) {
        debug!("TimelineUpdated: player {}", player_id);
        // Update local state
        if let Some(player) = self.players.get_mut(&player_id) {
            player.state.timeline = Some(timeline.clone());
        }
        // Directly apply only the timeline to devices currently showing this player
        for (device_id, device) in self.connected_devices.iter() {
            let is_selected = {
                let device = device.lock().unwrap();
                device.player_id == Some(player_id)
            };
            if is_selected {
                // best-effort; ignore errors here like other handlers
                self.applier.apply_timeline(device_id.clone(), Some(timeline.clone())).await.ok();
            }
        }
        // Do not mark devices for full update; no selection recompute needed for timeline-only changes
    }

    async fn handle_player_text_metadata_updated(&mut self, player_id: ManagedPlayerId, metadata: FsctTextMetadata, text: Option<String>) {
        debug!("TextMetadataUpdated: player {} {:?}", player_id, metadata);
        // Convert Option<String> to Option<&str> for apply_text
        let has_metadata_changed = if let Some(player) = self.players.get_mut(&player_id) {
            let had_metadata = player.has_metadata();
            *player.state.texts.get_mut_text(metadata) = text.clone();
            let has_metadata = player.has_metadata();
            has_metadata != had_metadata
        } else {
            return;
        };

        if has_metadata_changed {
            self.update_selected_players_for_devices(); // selection may change if metadata is now present or absent
            self.mark_devices_require_update_by_player(player_id);
            self.apply_on_devices_requiring_update().await;
        } else {
            // Directly apply only the text metadata to devices currently showing this player
            for (device_id, device) in self.connected_devices.iter() {
                let is_selected = {
                    let device = device.lock().unwrap();
                    device.player_id == Some(player_id)
                };
                if is_selected {
                    // best-effort; ignore errors here like other handlers
                    let text_ref = text.as_deref();
                    self.applier.apply_text(*device_id, metadata, text_ref).await.ok();
                }
            }
        }
    }

    // Dedicated handlers for DeviceEvent variants
    async fn handle_device_added(&mut self, device_id: ManagedDeviceId) {
        debug!("Device added: {}", device_id);
        self.connected_devices.insert(device_id, Mutex::new(ConnectedDevice::default()));
        let device = self.connected_devices.get(&device_id).unwrap();
        self.update_selected_player_for_device(&device_id, &device);
        let state = self.get_state_for_device(&device.lock().unwrap());
        self.applier.apply_to_device(device_id, &state).await.ok();
    }

    async fn handle_device_removed(&mut self, device_id: ManagedDeviceId) {
        debug!("Device removed: {}", device_id);
        self.connected_devices.remove(&device_id);
        self.applier.clean_cache_for_device(device_id);
    }

    // Selection helpers
    fn find_player_for_device(&self, device_id: &ManagedDeviceId, device: &Mutex<ConnectedDevice>) -> Option<(ManagedPlayerId, isize)> {
        let mut selected: Option<(ManagedPlayerId, isize)> = None;
        let last_selected = device.lock().unwrap().player_id.clone();
        for (player_id, player) in self.players.iter() {
            let is_last_selected = last_selected.as_ref().map(|id| *id == *player_id).unwrap_or(false);
            let player_score = player.score_for_player_and_device(device_id, is_last_selected);
            if scoring::is_better_selection(player_score, selected.map(|s| s.1).unwrap_or(0)) {
                selected = Some((*player_id, player_score));
            }
        }
        selected
    }


    fn update_selected_players_for_devices(&self) {
        for (device_id, device) in self.connected_devices.iter() {
            self.update_selected_player_for_device(device_id, device);
        }
    }

    fn update_selected_player_for_device(&self, device_id: &ManagedDeviceId, device: &Mutex<ConnectedDevice>) {
        let selected = self.find_player_for_device(device_id, device);
        let mut device = device.lock().unwrap();
        if device.player_id != selected.map(|s| s.0) {
            let score = selected.map(|s| s.1).unwrap_or(0);
            debug!("Selected player for device {} changed from {:?} to {:?}. New score: {}", device_id, device.player_id, selected, score);
            device.player_id = selected.map(|s| s.0);
            device.requires_update = true;
        }
    }

    async fn apply_on_devices_requiring_update(&self) {
        for (device_id, device) in self.connected_devices.iter() {
            let state = {
                let mut device = device.lock().unwrap();
                if device.requires_update {
                    let state = self.get_state_for_device(&device);
                    device.requires_update = false;
                    Some(state)
                } else {
                    None
                }
            };
            if let Some(state) = state {
                self.applier.apply_to_device(device_id.clone(), &state).await.ok();
            }
        }
    }

    fn get_state_for_device(&self, device: &ConnectedDevice) -> PlayerState {
        let state = device.player_id.as_ref()
            .map(|id| self.players.get(id))
            .flatten()
            .map(|p| p.state.clone())
            .unwrap_or_default();
        state
    }
}

