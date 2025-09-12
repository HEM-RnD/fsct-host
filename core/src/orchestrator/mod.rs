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

use std::cmp::{PartialOrd};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use log::{debug, info, warn};
use tokio::select;
use tokio::sync::broadcast;
use crate::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use crate::device_manager::{DeviceEvent, DeviceManager, ManagedDeviceId};
use crate::device_manager::DeviceControl;
use crate::player_events::PlayerEvent;
use crate::player_manager::ManagedPlayerId;
use crate::player_state::PlayerState;
use crate::player_state_applier::{DirectDeviceControlApplier, PlayerStateApplier};
use crate::service::{ServiceHandle, spawn_service};

#[derive(Debug, Clone, Default)]
struct RegisteredPlayer {
    assigned_device: Option<ManagedDeviceId>,
    state: PlayerState,
    is_assigned_device_attached: bool,
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
    pub fn run(mut self) -> ServiceHandle {
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
            player.is_assigned_device_attached = self.connected_devices.contains_key(&device_id);
        }

        self.update_selected_players_for_devices();
        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_player_unassigned(&mut self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) {
        debug!("Unassigned: player {} -/-> device {}", player_id, device_id);

        if let Some(player) = self.players.get_mut(&player_id) {
            player.assigned_device = None;
            player.is_assigned_device_attached = false;
        }

        self.update_selected_players_for_devices();

        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_player_state_updated(&mut self, player_id: ManagedPlayerId, state: PlayerState) {
        debug!("StateUpdated: player {}", player_id);

        let mut status_changed = false;

        if let Some(player) = self.players.get_mut(&player_id) {
            if player.state.status != state.status {
                status_changed = true;
            }
            player.state = state;
        }

        if status_changed {
            self.update_selected_players_for_devices();
        }
        for device in self.connected_devices.values() {
            let mut device = device.lock().unwrap();
            if device.player_id == Some(player_id) {
                device.requires_update = true;
            }
        }
        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_player_status_updated(&mut self, player_id: ManagedPlayerId, status: FsctStatus) {
        debug!("StatusUpdated: player {} -> {:?}", player_id, status);
        if let Some(player) = self.players.get_mut(&player_id) {
            player.state.status = status;
        }
        // Status change can affect selection
        self.update_selected_players_for_devices();
        // Mark devices currently showing this player for update
        for device in self.connected_devices.values() {
            let mut device = device.lock().unwrap();
            if device.player_id == Some(player_id) {
                device.requires_update = true;
            }
        }
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
        let text_ref = text.as_deref();
        // Directly apply only the specific text to devices currently showing this player
        for (device_id, device) in self.connected_devices.iter() {
            let is_selected = {
                let device = device.lock().unwrap();
                device.player_id == Some(player_id)
            };
            if is_selected {
                self.applier.apply_text(device_id.clone(), metadata, text_ref).await.ok();
            }
        }
        // Update local state after applies
        if let Some(player) = self.players.get_mut(&player_id) {
            let slot = player.state.texts.get_mut_text(metadata);
            *slot = text;
        }
        // Do not trigger full apply
    }

    // Dedicated handlers for DeviceEvent variants
    async fn handle_device_added(&mut self, device_id: ManagedDeviceId) {
        debug!("Device added: {}", device_id);
        self.connected_devices.insert(device_id, Mutex::new(ConnectedDevice::default()));
        for player in self.players.values_mut() {
            if player.assigned_device == Some(device_id) {
                player.is_assigned_device_attached = true;
            }
        }
        self.update_selected_players_for_devices();
        self.apply_on_devices_requiring_update().await;
    }

    async fn handle_device_removed(&mut self, device_id: ManagedDeviceId) {
        debug!("Device removed: {}", device_id);
        self.connected_devices.remove(&device_id);
        for player in self.players.values_mut() {
            if player.assigned_device == Some(device_id) {
                player.is_assigned_device_attached = false;
            }
        }
        // Players previously assigned to this device may now fall back to general group if no other connected device
        self.update_selected_players_for_devices();
        self.apply_on_devices_requiring_update().await;
    }

    // Selection helpers
    fn find_player_for_device(&self, device_id: &ManagedDeviceId) -> Option<ManagedPlayerId> {
        let mut selected = None;
        let mut selected_params = None;
        let last_selected = self.connected_devices.get(device_id)?.lock().unwrap().player_id.clone();
        for (player_id, player) in self.players.iter() {
            let assignment_state = if player.assigned_device.as_ref() == Some(device_id) {
                Assignment::AssignedToThisDevice
            } else if player.is_assigned_device_attached {
                Assignment::AssignedToOtherDevice
            } else {
                Assignment::Unassigned
            };
            let player_selection_params = PlayerSelectionParams {
                status: player.state.status.into(),
                is_last_selected: last_selected.map(|id| id == *player_id).unwrap_or(false),
                assignment: assignment_state,
            };
            if is_better_selection(&player_selection_params, &selected_params) {
                selected = Some(*player_id);
                selected_params = Some(player_selection_params);
            }
        }
        selected
    }

    fn update_selected_players_for_devices(&self) {
        for (device_id, device) in self.connected_devices.iter() {
            let selected = self.find_player_for_device(device_id);
            let mut device = device.lock().unwrap();
            if device.player_id != selected {
                device.player_id = selected;
                device.requires_update = true;
            }
        }
    }

    async fn apply_on_devices_requiring_update(&self) {
        for (device_id, device) in self.connected_devices.iter() {
            let state = {
                let mut device = device.lock().unwrap();
                if device.requires_update {
                    let state = device.player_id.as_ref()
                        .map(|id| self.players.get(id))
                        .flatten()
                        .map(|p| p.state.clone())
                        .unwrap_or_default();
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
}


#[derive(PartialEq, Eq, Clone, Copy, Debug, PartialOrd)]
enum Assignment {
    /// Player is assigned to a connected device, but it is not this device
    AssignedToOtherDevice,
    /// Player is not assigned to any device nor preferred by OS/user
    Unassigned,
    /// Player is assigned to a processed device
    AssignedToThisDevice,
}

impl Assignment {
    fn score(&self) -> isize {
        match self {
            Assignment::AssignedToOtherDevice => ASSIGNED_TO_OTHER_DEVICE_SCORE,
            Assignment::Unassigned => UNASSIGNED_SCORE,
            Assignment::AssignedToThisDevice => ASSIGNED_TO_THIS_DEVICE_SCORE,
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlaybackStatus {
    fn score(&self) -> isize {
        match self {
            Self::Playing => PLAYING_SCORE,
            Self::Paused => PAUSED_SCORE,
            Self::Stopped => STOPPED_SCORE,
        }
    }
}

impl From<FsctStatus> for PlaybackStatus {
    fn from(status: FsctStatus) -> Self {
        match status {
            FsctStatus::Playing => Self::Playing,
            FsctStatus::Paused => Self::Paused,
            _ => Self::Stopped,
        }
    }
}

const PLAYING_SCORE: isize = 16;
const PAUSED_SCORE: isize = 4;
const STOPPED_SCORE: isize = 3;

const ASSIGNED_TO_OTHER_DEVICE_SCORE: isize = 0;
const UNASSIGNED_SCORE: isize = 4;
// const USER_SELECTED_SCORE: isize = 2;
const ASSIGNED_TO_THIS_DEVICE_SCORE: isize = 5;
const IS_LAST_SELECTED_SCORE: isize = 9;

//this is for reference and tests only:
const PLAYER_SELECTION_PARAMS_ALL_COMBINATIONS: [PlayerSelectionParams; 18] = [
    // when assigned to other device: they are not relevant at all, so we return 0
    PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false }, //0
    PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToOtherDevice, is_last_selected: true }, //0
    PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false }, //0
    PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::AssignedToOtherDevice, is_last_selected: true }, //0
    PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false }, //0
    PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToOtherDevice, is_last_selected: true }, //0

    // we should go here when there is no last selected player, so we are actually trying to find the best one
    PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: false }, // 1 (unassigned - 1)
    PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToThisDevice, is_last_selected: false }, // 2 (assigned - 2)
    // not sure if paused is better than assigned, but maybe it doesn't matter here
    PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::Unassigned, is_last_selected: false }, //3 (unassigned - 1, paused - 2)
    PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::AssignedToThisDevice, is_last_selected: false }, //4 (assigned - 2, paused - 2)

    // when paused or stopped, we prefer last selected player over others assuming that there is only one last selected player, order of other
    // parameters doesn't matter
    PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: true }, // 5 (unassigned - 1, is selected - 4)
    PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToThisDevice, is_last_selected: true }, // 6 (assigned - 2, is selected - 4
    PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::Unassigned, is_last_selected: true }, // 7 (unassigned - 1, paused - 2, is selected - 4)
    PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::AssignedToThisDevice, is_last_selected: true }, // 8 (assigned - 2, paused - 2, is selected - 4)

    // Playing are more prefered than not playing (if not assigned to another device)
    PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: false }, // 9 (unassigned - 1, playing - 8)
    PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: true }, // 10 (unassigned - 1, playing - 8, is selected - 1)
    PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToThisDevice, is_last_selected: false }, // 11 (assigned - 2, playing - 8)
    PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToThisDevice, is_last_selected: true },  // 12 (assigned - 2, playing - 8, is selected - 1)
];


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerSelectionParams {
    status: PlaybackStatus,
    assignment: Assignment,
    is_last_selected: bool,
}


impl PlayerSelectionParams {
    fn score(&self) -> isize {
        if self.assignment == Assignment::AssignedToOtherDevice {
            // special case: when assigned to the other device, they are not relevant at all, so we return 0
            return 0;
        }

        let mut score = 0;
        score += self.status.score() * self.assignment.score();
        score += self.is_last_selected.then_some(IS_LAST_SELECTED_SCORE).unwrap_or(0);
        score
    }
}


fn is_better_selection(player_params: &PlayerSelectionParams, current_selection: &Option<PlayerSelectionParams>) -> bool {
    match (current_selection, player_params) {
        (_, player) if player.assignment != Assignment::AssignedToOtherDevice => false, // ignore devices assigned to other devices
        (None, _) => true, // any better than nothing
        (Some(current), player) => {
            let current_score = current.score();
            let player_score = player.score();
            player_score > current_score
        }
    }
}
