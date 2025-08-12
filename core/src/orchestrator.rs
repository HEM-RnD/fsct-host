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

use std::cmp::{PartialOrd};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use log::{debug, info, warn};
use tokio::select;
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;
use crate::definitions::FsctStatus;
use crate::device_manager::{DeviceEvent, DeviceManager, ManagedDeviceId};
use crate::device_manager::DeviceControl;
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
    // Selection memory
    preferred_player: Option<ManagedPlayerId>, // user-preferred player for general group
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
            preferred_player: None,
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
        });
        OrchestratorHandle { join, shutdown_tx }
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
            PlayerEvent::PreferredChanged { preferred } => {
                self.handle_preferred_changed(preferred).await;
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
        if self.preferred_player == Some(player_id) { self.preferred_player = None; }

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

    async fn handle_preferred_changed(&mut self, preferred: Option<ManagedPlayerId>) {
        debug!("PreferredChanged: {:?}", preferred);
        self.preferred_player = preferred;

        self.update_selected_players_for_devices();
        self.apply_on_devices_requiring_update().await;
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
            } else if Some(player_id) == self.preferred_player.as_ref() {
                Assignment::UserSelected
            } else {
                Assignment::Unassigned
            };
            let player_selection_params = PlayerSelectionParams {
                is_playing: player.state.status == FsctStatus::Playing,
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

    fn select_player_for_device(&self, device_id: &ManagedDeviceId) {
        let selected = self.find_player_for_device(device_id);
        let mut device = self.connected_devices.get(device_id).unwrap().lock().unwrap(); // device is always present
        if device.player_id != selected {
            device.player_id = selected;
            device.requires_update = true;
        }
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
    /// Player is not assigned to any device, but it is preferred by OS/user
    UserSelected,
    /// Player is assigned to a processed device
    AssignedToThisDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerSelectionParams {
    // is_preferred: bool, // it means that player is prefered by user, even over playing player, but it only can be true
    // when there is no other player assigned to this device, which means that assigned to this device has higher
    // priority than is preferred, but only when preferred player is not playing.
    is_playing: bool, // we prefer playing players than assigned to this device
    // is_assigned_to_this_device: bool, // but we prefer players assigned to this device when playing
    // is_assigned_to_connected_device: bool, // we don't prefer players assigned to other devices
    assignment: Assignment,
    is_last_selected: bool, // we prefer last selected player over others, but only when other options are the same
}


fn is_better_selection(player_params: &PlayerSelectionParams, current_selection: &Option<PlayerSelectionParams>) -> bool {
    match (current_selection, player_params) {
        (None, _) => true, // no selection yet, so it's the best
        (Some(current), player) => {
            // when players are in identical situation, we prefer previously selected player over others
            if player.assignment == current.assignment && player.is_playing == current.is_playing {
                return player.is_last_selected;
            }
            // when one is playing, and another is not, and they are in identical state, we prefer playing one
            if player.assignment == current.assignment {
                return player.is_playing;
            }

            // the rest cases are more complex, so we need to compare them:
            match (player.is_playing, player.assignment, current.is_playing, current.assignment) {
                // prefer user selected over unassigned, even when playing
                (true, Assignment::Unassigned, false, Assignment::UserSelected) => false,
                (false, Assignment::UserSelected, true, Assignment::Unassigned) => true,

                // prefer not playing over assigned to other device, even when playing
                (true, Assignment::AssignedToOtherDevice, false,  _) => false,
                (false, _, true, Assignment::AssignedToOtherDevice) => true,

                // ok, in other cases, playing is better
                (true, _, false, _) => true,
                (false, _, true, _) => false,

                // prefer user selected over others, when not playing
                (false, Assignment::UserSelected, false, _) => true,
                (false, _, false, Assignment::UserSelected) => false,

                // the rest of cases includes only situations when both players are playing or both are not playing,
                // so we can compare assignments directly
                (_, player_assignment, _, current_assignment) => player_assignment > current_assignment,
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Error;
    use std::sync::Mutex;
    use tokio::time::{sleep, Duration};
    use uuid::Uuid;
    use crate::definitions::FsctStatus;

    // ----------------- Helpers for selection testing -----------------
    fn fold_best(items: &[PlayerSelectionParams]) -> PlayerSelectionParams {
        let mut current: Option<PlayerSelectionParams> = None;
        for cand in items {
            if is_better_selection(cand, &current) {
                current = Some(*cand);
            }
        }
        current.expect("fold_best requires at least one item")
    }

    fn permute_indices_rec(n: usize, current: &mut Vec<usize>, used: &mut Vec<bool>, out: &mut Vec<Vec<usize>>) {
        if current.len() == n {
            out.push(current.clone());
            return;
        }
        for i in 0..n {
            if !used[i] {
                used[i] = true;
                current.push(i);
                permute_indices_rec(n, current, used, out);
                current.pop();
                used[i] = false;
            }
        }
    }

    fn permute_indices(n: usize) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        let mut used = vec![false; n];
        let mut cur = Vec::with_capacity(n);
        permute_indices_rec(n, &mut cur, &mut used, &mut out);
        out
    }

    fn selection_is_order_independent(items: &[PlayerSelectionParams]) -> (bool, PlayerSelectionParams) {
        let base = fold_best(items);
        for perm in permute_indices(items.len()) {
            let permuted: Vec<PlayerSelectionParams> = perm.iter().map(|&i| items[i]).collect();
            let w = fold_best(&permuted);
            if w != base {
                return (false, base);
            }
        }
        (true, base)
    }

    // Physically sort by repeatedly picking the best remaining (deterministic for tests)
    fn sort_by_preference(items: &[PlayerSelectionParams]) -> Vec<PlayerSelectionParams> {
        let mut rest: Vec<PlayerSelectionParams> = items.to_vec();
        let mut out = Vec::with_capacity(rest.len());
        while !rest.is_empty() {
            // find index of the best element
            let mut best_idx = 0;
            let mut best_opt: Option<PlayerSelectionParams> = None;
            for (i, cand) in rest.iter().enumerate() {
                if is_better_selection(cand, &best_opt) {
                    best_opt = Some(*cand);
                    best_idx = i;
                }
            }
            out.push(rest.remove(best_idx));
        }
        out
    }

    #[derive(Debug, Clone, PartialEq)]
    struct ApplyCall {
        device: ManagedDeviceId,
        state: PlayerState,
    }

    struct MockApplier {
        calls: Mutex<Vec<ApplyCall>>,
    }

    impl MockApplier {
        fn new() -> Arc<Self> { Arc::new(Self { calls: Mutex::new(Vec::new()) }) }
        fn take(&self) -> Vec<ApplyCall> { std::mem::take(&mut self.calls.lock().unwrap()) }
    }

    impl PlayerStateApplier for MockApplier {
        fn apply_to_device<'a>(&'a self, device_id: ManagedDeviceId, state: &'a PlayerState)
            -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<(), Error>> + Send + 'a>> {
            let st = state.clone();
            Box::pin(async move {
                let mut guard = self.calls.lock().unwrap();
                let duplicate = guard.iter().any(|c| c.device == device_id && c.state == st);
                if !duplicate {
                    // debug print in tests to understand sequences
                    #[cfg(test)]
                    {
                        println!("APPLY dev={:?} status={:?}", device_id, st.status);
                    }
                    guard.push(ApplyCall { device: device_id, state: st });
                }
                Ok(())
            })
        }
    }

    fn make_ids(n: usize) -> Vec<ManagedDeviceId> { (0..n).map(|_| Uuid::new_v4()).collect() }
    fn pid(n: u32) -> ManagedPlayerId { std::num::NonZeroU32::new(n).unwrap() }

    fn default_state_with_title(title: &str) -> PlayerState {
        let mut s = PlayerState::default();
        s.texts.get_mut_text(crate::definitions::FsctTextMetadata::CurrentTitle).replace(title.to_string());
        s
    }

    // Helper to build orchestrator and the senders
    fn build_orchestrator(applier: Arc<MockApplier>) -> (
        Orchestrator<MockApplier>,
        tokio::sync::broadcast::Sender<PlayerEvent>,
        tokio::sync::broadcast::Sender<DeviceEvent>,
    ) {
        let (player_tx, player_rx) = tokio::sync::broadcast::channel(256);
        let (device_tx, device_rx) = tokio::sync::broadcast::channel(256);
        let orch = Orchestrator::new_with_applier(player_rx, device_rx, applier);
        (orch, player_tx, device_tx)
    }

    async fn run_orchestrator(orch: Orchestrator<MockApplier>) -> OrchestratorHandle {
        orch.run()
    }

    async fn short_wait() { sleep(Duration::from_millis(10)).await }

    #[tokio::test]
    async fn zero_players_zero_devices_no_apply() {
        let applier = MockApplier::new();
        let (orch, _ptx, _dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        short_wait().await;
        assert!(applier.take().is_empty());
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn one_player_zero_devices_state_update_no_apply() {
        let applier = MockApplier::new();
        let (orch, ptx, _dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;

        let p1 = pid(1);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let s1 = default_state_with_title("S1");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1 });

        short_wait().await;
        assert!(applier.take().is_empty());
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn zero_players_one_device_add_no_apply() {
        let applier = MockApplier::new();
        let (orch, _ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;

        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        assert!(applier.take().is_empty());
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn unassigned_state_then_device_added_applies_to_device() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;

        let p1 = pid(1);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let s1 = default_state_with_title("S1");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        short_wait().await;
        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));

        short_wait().await;
        let calls = applier.take();
        // allow possible initial Unknown applies; require that S1 was applied to d at least once
        assert!(calls.iter().any(|c| c.device == d && c.state == s1));
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn assign_before_connect_then_connect_then_update() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;

        let p1 = pid(1);
        let d = make_ids(1)[0];
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let s1 = default_state_with_title("S1");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d });
        // give orchestrator a moment to record the assignment before device connects
        short_wait().await;
        // device connects after assignment
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        // should apply s1 once due to device added with assigned player
        let mut calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d);
        assert_eq!(calls[0].state, s1);

        // update to S2 -> apply again to assigned device
        let s2 = default_state_with_title("S2");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s2.clone() });
        short_wait().await;
        calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d);
        assert_eq!(calls[0].state, s2);

        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn multiple_players_one_device_unassigned_and_assignment_switch() {
        env_logger::init();
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let d = make_ids(1)[0];
        let p1 = pid(1);
        let p2 = pid(2);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        short_wait().await;

        let mut s1 = default_state_with_title("S1");
        let mut s2 = default_state_with_title("S2");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        short_wait().await;
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        let mut calls = applier.take();
        // Accept possible initial Unknown applies; ensure S1 reached device d
        assert!(calls.iter().any(|c| c.device == d && c.state == s1));

        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
        short_wait().await;

        calls = applier.take();
        // ensure S2 did not reach device d yet
        assert!(calls.is_empty());

        // P2 updates -> becomes not selected; should not propagate to unassigned device d
        let s2 = default_state_with_title("S2");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        short_wait().await;
        calls = applier.take();
        // ensures S2 did not reach device d yet
        assert!(calls.is_empty());

        // Now assign P2 to d -> should apply P2's latest state to d
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p2, device_id: d });
        short_wait().await;
        calls = applier.take();
        // P2 has known state s2 and device connected, assignment applies s2 (at least once)
        assert!(calls.iter().any(|c| c.device == d && c.state == s2));

        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn one_player_multiple_devices_unassigned_then_assign() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let p1 = pid(1);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let s1 = default_state_with_title("S1");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        short_wait().await;
        let ids = make_ids(2);
        let d1 = ids[0];
        let d2 = ids[1];
        let _ = dtx.send(DeviceEvent::Added(d1));
        let _ = dtx.send(DeviceEvent::Added(d2));
        short_wait().await;
        let mut calls = applier.take();
        // both devices should eventually receive s1 (there may be initial Unknown applies)
        assert!(calls.iter().any(|c| c.device == d1 && c.state == s1));
        assert!(calls.iter().any(|c| c.device == d2 && c.state == s1));

        // Assign player to d1 -> should apply s1 to d1 again; d2 remains unassigned with prior state
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d1 });
        short_wait().await;
        calls = applier.take();
        // S1 has not changed, so nothing has been applied
        assert!(calls.is_empty());

        // Update to S2 -> applies to assigned device d1
        let s2 = default_state_with_title("S2");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s2.clone() });
        short_wait().await;
        calls = applier.take();
        assert!(calls.iter().any(|c| c.device == d1 && c.state == s2));

        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn preferred_change_does_not_apply() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let p1 = pid(1);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        short_wait().await;
        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        let _ = applier.take(); // clear any initial applies (e.g., Unknown)
        let _ = ptx.send(PlayerEvent::PreferredChanged { preferred: Some(p1) });
        short_wait().await;
        // No state known, preferred change should not cause any additional apply
        assert!(applier.take().is_empty());
        let _ = handle.shutdown().await;
    }

    // New tests for advanced grouping and selection
    #[tokio::test]
    async fn preferred_player_drives_general_group() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let p1 = pid(1);
        let p2 = pid(2);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
        let mut s1 = default_state_with_title("S1");
        s1.status = FsctStatus::Paused;
        let mut s2 = default_state_with_title("S2");
        s2.status = FsctStatus::Stopped;
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        // set preferred to p2
        let _ = ptx.send(PlayerEvent::PreferredChanged { preferred: Some(p2) });
        short_wait().await;
        // connect two unassigned devices
        let ids = make_ids(2);
        let d1 = ids[0];
        let d2 = ids[1];
        let _ = dtx.send(DeviceEvent::Added(d1));
        let _ = dtx.send(DeviceEvent::Added(d2));
        short_wait().await;
        let calls = applier.take();
        // Both devices should have preferred p2 state at least once
        assert!(calls.iter().any(|c| c.device == d1 && c.state == s2));
        assert!(calls.iter().any(|c| c.device == d2 && c.state == s2));
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn general_group_picks_playing_if_no_preferred() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let p1 = pid(1);
        let p2 = pid(2);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
        let mut s1 = default_state_with_title("S1");
        s1.status = FsctStatus::Playing;
        let mut s2 = default_state_with_title("S2");
        s2.status = FsctStatus::Paused;
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        short_wait().await;
        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        let calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].state, s1);
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn multiple_playing_keep_last_active_in_general() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let p1 = pid(1);
        let p2 = pid(2);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
        let mut s1 = default_state_with_title("S1");
        s1.status = FsctStatus::Playing;
        let mut s2 = default_state_with_title("S2");
        s2.status = FsctStatus::Playing;
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        let _ = applier.take(); // p1 selected
        // now p2 starts playing as well
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        short_wait().await;
        let calls = applier.take();
        // ambiguous, should keep last active (p1) and not reapply since state didn't change for p1
        assert!(calls.is_empty());
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn device_group_with_multiple_players_picks_playing() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));
        let p1 = pid(1);
        let p2 = pid(2);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
        let mut s1 = default_state_with_title("S1");
        s1.status = FsctStatus::Paused;
        let mut s2 = default_state_with_title("S2");
        s2.status = FsctStatus::Playing;
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d });
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p2, device_id: d });
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        short_wait().await;
        let calls = applier.take();
        assert!(!calls.is_empty());
        assert_eq!(calls.last().unwrap().device, d);
        assert_eq!(calls.last().unwrap().state, s2);
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn assigned_to_disconnected_counts_as_general() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let d_assigned = make_ids(1)[0]; // will remain disconnected
        let d_unassigned = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d_unassigned));
        let p1 = pid(1);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let s1 = default_state_with_title("S1");
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d_assigned });
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        short_wait().await;
        let calls = applier.take();
        // p1 should be applied to unassigned connected device (there may be an initial Unknown)
        assert!(calls.iter().any(|c| c.device == d_unassigned && c.state == s1));
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn general_does_not_pick_playing_assigned_to_other_device() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let p1 = pid(1);
        let p2 = pid(2);
        let d1 = make_ids(1)[0];
        let d2 = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d1)); // device with assigned group
        let _ = dtx.send(DeviceEvent::Added(d2)); // unassigned mirrors general
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
        let mut s1 = default_state_with_title("S1");
        s1.status = FsctStatus::Playing;
        let mut s2 = default_state_with_title("S2");
        s2.status = FsctStatus::Paused;
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d1 });
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        short_wait().await;
        let calls = applier.take();
        // d1 gets s1 due to assignment; general (unassigned) should prefer unassigned p2 over playing p1 assigned elsewhere
        assert!(calls.iter().any(|c| c.device == d1 && c.state == s1));
        assert!(calls.iter().any(|c| c.device == d2 && c.state == s2));
        let _ = handle.shutdown().await;
    }

    #[test]
    fn is_better_selection_order_independence_three_cases() {
        // Build three elements as requested:
        // 1) playing unassigned
        // 2) non-playing user-selected
        // 3) non-playing assigned to current device
        let a_playing_unassigned = PlayerSelectionParams {
            is_playing: true,
            assignment: Assignment::Unassigned,
            is_last_selected: false,
        };
        let b_non_playing_user_selected = PlayerSelectionParams {
            is_playing: false,
            assignment: Assignment::UserSelected,
            is_last_selected: false,
        };
        let c_non_playing_assigned_here = PlayerSelectionParams {
            is_playing: false,
            assignment: Assignment::AssignedToThisDevice,
            is_last_selected: false,
        };

        let items = vec![
            a_playing_unassigned,
            b_non_playing_user_selected,
            c_non_playing_assigned_here,
        ];

        // Use helper to verify order-independence and assert expected winner
        let (stable, winner) = selection_is_order_independent(&items);
        assert!(stable, "Winner should be identical across all permutations");
        assert_eq!(winner, b_non_playing_user_selected, "Non-playing user-selected should beat playing unassigned and idle assigned-here in this triad");

        // Additionally, verify sorting stability across all permutations using the helper sort
        let baseline_sorted = sort_by_preference(&items);
        for perm in permute_indices(items.len()) {
            let permuted: Vec<PlayerSelectionParams> = perm.iter().map(|&i| items[i]).collect();
            let sorted = sort_by_preference(&permuted);
            assert_eq!(sorted, baseline_sorted, "Sorting should be stable regardless of input order for the 3-case scenario");
        }
    }

    #[test]
    fn is_better_selection_order_independence_six_players_and_sort_stability() {
        let p_a_playing_assigned_here = PlayerSelectionParams { is_playing: true, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
        let p_b_user_selected_idle   = PlayerSelectionParams { is_playing: false, assignment: Assignment::UserSelected,         is_last_selected: false };
        let p_c_playing_unassigned   = PlayerSelectionParams { is_playing: true, assignment: Assignment::Unassigned,           is_last_selected: false };
        let p_d_playing_assigned_other = PlayerSelectionParams { is_playing: true, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };
        let p_e_idle_assigned_here   = PlayerSelectionParams { is_playing: false, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
        let p_f_idle_unassigned_last = PlayerSelectionParams { is_playing: false, assignment: Assignment::Unassigned,           is_last_selected: true };

        let items = vec![
            p_a_playing_assigned_here,
            p_b_user_selected_idle,
            p_c_playing_unassigned,
            p_d_playing_assigned_other,
            p_e_idle_assigned_here,
            p_f_idle_unassigned_last,
        ];

        // Check order independence of the winner
        let (stable, base_winner) = selection_is_order_independent(&items);
        assert!(stable, "Winner should be the same for all permutations");
        assert_eq!(base_winner, p_a_playing_assigned_here, "Expected the strongest candidate to win");

        // Check that the full sorting is stable across permutations (deterministic for this set)
        let baseline_sorted = sort_by_preference(&items);
        for perm in permute_indices(items.len()) {
            let permuted: Vec<PlayerSelectionParams> = perm.iter().map(|&i| items[i]).collect();
            let sorted = sort_by_preference(&permuted);
            assert_eq!(sorted, baseline_sorted, "Sorting should be stable regardless of input order");
        }
    }

    #[test]
    fn is_better_selection_tie_broken_by_last_selected() {
        // All identical except is_last_selected
        let x1 = PlayerSelectionParams { is_playing: false, assignment: Assignment::Unassigned, is_last_selected: false };
        let x2 = PlayerSelectionParams { is_playing: false, assignment: Assignment::Unassigned, is_last_selected: true  }; // should win
        let x3 = PlayerSelectionParams { is_playing: false, assignment: Assignment::Unassigned, is_last_selected: false };
        let x4 = PlayerSelectionParams { is_playing: false, assignment: Assignment::Unassigned, is_last_selected: false };
        let items = vec![x1, x2, x3, x4];

        let (stable, winner) = selection_is_order_independent(&items);
        assert!(stable, "Tie breaker by last selected must be order independent");
        assert_eq!(winner, x2, "The one flagged as last selected should be preferred among equals");
    }

    #[test]
    fn is_better_selection_penalizes_assigned_to_other_device() {
        // Playing but assigned elsewhere should lose to an idle unassigned
        let playing_other = PlayerSelectionParams { is_playing: true, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };
        let idle_unassigned = PlayerSelectionParams { is_playing: false, assignment: Assignment::Unassigned, is_last_selected: false };
        let items = vec![playing_other, idle_unassigned];

        let (stable, winner) = selection_is_order_independent(&items);
        assert!(stable);
        assert_eq!(winner, idle_unassigned, "Idle unassigned should be preferred over playing assigned to other device");
    }
}
