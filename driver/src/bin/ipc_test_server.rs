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

//! Minimal IPC server backed by a mock FsctDriver, used for Node.js integration tests.
//!
//! Usage: ipc_test_server <socket-path>
//!
//! Starts the IPC server on the given socket path and runs until killed.
//! The first client connection triggers a device_changed notification after 200ms.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use async_trait::async_trait;
use fsct::definitions::{DeviceInfo, FsctStatus, FsctTextMetadata, ManagedDeviceId, ManagedPlayerId, TimelineInfo};
use fsct::player_state::PlayerState;
use fsct::{DeviceChangeEvent, FsctDriver};
use fsct_driver::IpcServer;
use tokio::sync::broadcast;
use uuid::Uuid;

const DEVICE_ID_1: Uuid = uuid::uuid!("11111111-0000-0000-0000-000000000001");
const DEVICE_ID_2: Uuid = uuid::uuid!("22222222-0000-0000-0000-000000000002");

struct MockFsctDriver {
    next_id: AtomicU32,
    assignments: Mutex<HashMap<NonZeroU32, Uuid>>,
    device_changes_tx: broadcast::Sender<DeviceChangeEvent>,
    event_once: Once,
}

impl MockFsctDriver {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            next_id: AtomicU32::new(1),
            assignments: Mutex::new(HashMap::new()),
            device_changes_tx: tx,
            event_once: Once::new(),
        }
    }
}

#[async_trait]
impl FsctDriver for MockFsctDriver {
    async fn register_player(&self, _self_id: String) -> anyhow::Result<ManagedPlayerId> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        Ok(NonZeroU32::new(id).expect("player id counter wrapped to zero"))
    }

    async fn unregister_player(&self, player_id: ManagedPlayerId) -> anyhow::Result<()> {
        self.assignments.lock().unwrap().remove(&player_id);
        Ok(())
    }

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<()> {
        self.assignments.lock().unwrap().insert(player_id, device_id);
        Ok(())
    }

    async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<()> {
        let mut guard = self.assignments.lock().unwrap();
        if guard.get(&player_id) == Some(&device_id) {
            guard.remove(&player_id);
        }
        Ok(())
    }

    async fn update_player_state(&self, _player_id: ManagedPlayerId, _new_state: PlayerState) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update_player_status(&self, _player_id: ManagedPlayerId, _new_status: FsctStatus) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update_player_timeline(&self, _player_id: ManagedPlayerId, _new_timeline: Option<TimelineInfo>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update_player_metadata(
        &self,
        _player_id: ManagedPlayerId,
        _metadata_id: FsctTextMetadata,
        _new_text: Option<String>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>> {
        Ok(self.assignments.lock().unwrap().get(&player_id).copied())
    }

    async fn get_detected_devices(&self) -> anyhow::Result<Vec<ManagedDeviceId>> {
        Ok(vec![DEVICE_ID_1, DEVICE_ID_2])
    }

    async fn get_device_info(&self, device_id: ManagedDeviceId) -> anyhow::Result<DeviceInfo> {
        match device_id {
            DEVICE_ID_1 => Ok(DeviceInfo {
                id: DEVICE_ID_1,
                name: Some("FSCT Test Device 1".into()),
                manufacturer: Some("HEM Sp. z o.o.".into()),
                vendor_id: 0x1234,
                product_id: 0x0001,
                serial_number: Some("SN-TEST-001".into()),
            }),
            DEVICE_ID_2 => Ok(DeviceInfo {
                id: DEVICE_ID_2,
                name: Some("FSCT Test Device 2".into()),
                manufacturer: Some("HEM Sp. z o.o.".into()),
                vendor_id: 0x1234,
                product_id: 0x0002,
                serial_number: None,
            }),
            _ => Err(anyhow::anyhow!("device not found: {}", device_id)),
        }
    }

    async fn subscribe_device_changes(&self) -> anyhow::Result<broadcast::Receiver<DeviceChangeEvent>> {
        let rx = self.device_changes_tx.subscribe();
        // On the first subscription (first client connection), emit an Added event after 200ms.
        // This gives Node.js tests time to attach event listeners before the notification fires.
        let tx = self.device_changes_tx.clone();
        self.event_once.call_once(|| {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                let _ = tx.send(DeviceChangeEvent::Added(DEVICE_ID_1));
            });
        });
        Ok(rx)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let endpoint = std::env::args()
        .nth(1)
        .expect("Usage: ipc_test_server <socket-path>");

    let driver = Arc::new(MockFsctDriver::new());
    let mut server = IpcServer::with_socket_path(driver, &endpoint);

    tokio::select! {
        res = server.serve() => {
            if let Err(e) = res {
                eprintln!("ipc_test_server: server error: {e}");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            eprintln!("ipc_test_server: received Ctrl-C, shutting down");
        }
    }

    server.shutdown().await;
    Ok(())
}
