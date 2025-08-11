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

use log::{debug, info, warn};
use tokio::select;
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;

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

/// Orchestrator subscribes to PlayerManager and DeviceManager events
/// and applies routing policy to update devices using a PlayerStateApplier.
pub struct Orchestrator<A: PlayerStateApplier> {
    // Receivers
    player_rx: broadcast::Receiver<PlayerEvent>,
    device_rx: broadcast::Receiver<DeviceEvent>,

    // Applier that performs device I/O
    applier: Arc<A>,

    // Routing state
    player_to_device: HashMap<ManagedPlayerId, ManagedDeviceId>,
    device_to_player: HashMap<ManagedDeviceId, ManagedPlayerId>,
    connected_devices: HashSet<ManagedDeviceId>,
    last_state_per_player: HashMap<ManagedPlayerId, PlayerState>,
    active_unassigned: Option<ManagedPlayerId>, // policy: last updated among unassigned
    preferred_player: Option<ManagedPlayerId>, // user-preferred player (stored only; not applied yet)
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
            player_to_device: HashMap::new(),
            device_to_player: HashMap::new(),
            connected_devices: HashSet::new(),
            last_state_per_player: HashMap::new(),
            active_unassigned: None,
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
    }

    async fn handle_player_unregistered(&mut self, player_id: ManagedPlayerId) {
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
        // Clear preferred if it pointed to the unregistered player
        if self.preferred_player == Some(player_id) {
            self.preferred_player = None;
        }
    }

    async fn handle_player_assigned(&mut self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) {
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

    async fn handle_player_unassigned(&mut self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) {
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

    async fn handle_player_state_updated(&mut self, player_id: ManagedPlayerId, state: PlayerState) {
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

    async fn handle_preferred_changed(&mut self, preferred: Option<ManagedPlayerId>) {
        debug!("PreferredChanged: {:?}", preferred);
        self.preferred_player = preferred;
        // Intentionally no further action for now (policy changes to be added later)
    }

    // Dedicated handlers for DeviceEvent variants
    async fn handle_device_added(&mut self, device_id: ManagedDeviceId) {
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

    async fn handle_device_removed(&mut self, device_id: ManagedDeviceId) {
        debug!("Device removed: {}", device_id);
        self.connected_devices.remove(&device_id);
        if let Some(player_id) = self.device_to_player.remove(&device_id) {
            self.player_to_device.remove(&player_id);
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


#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Error;
    use std::sync::Mutex;
    use tokio::time::{sleep, Duration};
    use uuid::Uuid;

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
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>> {
            let st = state.clone();
            Box::pin(async move {
                self.calls.lock().unwrap().push(ApplyCall { device: device_id, state: st });
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
        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));

        short_wait().await;
        let calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d);
        assert_eq!(calls[0].state, s1);
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
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let d = make_ids(1)[0];
        let p1 = pid(1);
        let p2 = pid(2);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });

        let s1 = default_state_with_title("S1");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        let mut calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].state, s1);

        // P2 updates -> becomes active_unassigned; should propagate to unassigned device d
        let s2 = default_state_with_title("S2");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        short_wait().await;
        calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d);
        assert_eq!(calls[0].state, s2);

        // Now assign P1 to d -> should apply P1's latest state to d
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d });
        short_wait().await;
        calls = applier.take();
        // P1 has known state s1 and device connected, assignment applies s1
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d);
        assert_eq!(calls[0].state, s1);

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
        let ids = make_ids(2);
        let d1 = ids[0];
        let d2 = ids[1];
        let _ = dtx.send(DeviceEvent::Added(d1));
        let _ = dtx.send(DeviceEvent::Added(d2));
        short_wait().await;
        let mut calls = applier.take();
        // both devices should have received s1 (order may match send order)
        assert_eq!(calls.len(), 2);
        calls.sort_by_key(|c| c.device);
        let mut devices = vec![calls[0].device, calls[1].device];
        devices.sort();
        let mut expected = vec![d1, d2]; expected.sort();
        assert_eq!(devices, expected);
        assert_eq!(calls[0].state, s1);
        assert_eq!(calls[1].state, s1);

        // Assign player to d1 -> should apply s1 to d1 again; d2 remains unassigned with prior state
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d1 });
        short_wait().await;
        calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d1);
        assert_eq!(calls[0].state, s1);

        // Update to S2 -> applies only to assigned device d1
        let s2 = default_state_with_title("S2");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s2.clone() });
        short_wait().await;
        calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d1);
        assert_eq!(calls[0].state, s2);

        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn preferred_change_does_not_apply() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let p1 = pid(1);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let d = make_ids(1)[0];
        let _ = dtx.send(DeviceEvent::Added(d));
        let _ = ptx.send(PlayerEvent::PreferredChanged { preferred: Some(p1) });
        short_wait().await;
        // No state known, preferred change should not cause any apply
        assert!(applier.take().is_empty());
        let _ = handle.shutdown().await;
    }

    #[tokio::test]
    async fn unassign_propagates_active_unassigned_to_device() {
        let applier = MockApplier::new();
        let (orch, ptx, dtx) = build_orchestrator(applier.clone());
        let handle = run_orchestrator(orch).await;
        let d = make_ids(1)[0];
        let p1 = pid(1);
        let p2 = pid(2);
        let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
        let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
        let s1 = default_state_with_title("S1");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
        let _ = dtx.send(DeviceEvent::Added(d));
        short_wait().await;
        let _ = applier.take(); // clear initial apply to unassigned device
        // Assign P1 to d
        let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d });
        short_wait().await;
        let _ = applier.take(); // assignment may apply s1; clear
        // P2 updates -> becomes active_unassigned
        let s2 = default_state_with_title("S2");
        let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
        short_wait().await;
        let _ = applier.take(); // since d is assigned, no apply now
        // Unassign P1 from d -> should propagate active_unassigned (P2) to d
        let _ = ptx.send(PlayerEvent::Unassigned { player_id: p1, device_id: d });
        short_wait().await;
        let calls = applier.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].device, d);
        assert_eq!(calls[0].state, s2);
        let _ = handle.shutdown().await;
    }
}
