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
//! Stdio protocol (NDJSON):
//!   stdout → test harness:
//!     {"type":"ready"}                              — socket is bound and accepting
//!     {"type":"received","method":"update_player_status","playerId":N,"status":"..."}
//!     {"type":"received","method":"update_player_timeline","playerId":N,"timeline":{...}|null}
//!     {"type":"received","method":"update_player_metadata","playerId":N,"metadataId":"...","text":"..."|null}
//!     {"type":"received","method":"update_player_state","playerId":N,"state":{...}}
//!   stdin ← test harness:
//!     {"type":"emit_device_changed","event":"added"|"removed","deviceId":"UUID"}

use std::collections::HashMap;
use std::io::Write;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fsct::definitions::{
    DeviceInfo, FsctStatus, FsctTextMetadata, ManagedDeviceId, ManagedPlayerId, TimeSync, TimelineInfo,
};
use fsct::player_state::PlayerState;
use fsct::{DeviceChangeEvent, FsctDriver};
use fsct_driver::IpcServer;
use tokio::sync::broadcast;
use uuid::Uuid;

const DEVICE_ID_1: Uuid = uuid::uuid!("11111111-0000-0000-0000-000000000001");
const DEVICE_ID_2: Uuid = uuid::uuid!("22222222-0000-0000-0000-000000000002");

fn emit_received(value: serde_json::Value) {
    if let Ok(line) = serde_json::to_string(&value) {
        println!("{line}");
        let _ = std::io::stdout().flush();
    }
}

struct MockFsctDriver {
    next_id: AtomicU32,
    assignments: Mutex<HashMap<NonZeroU32, Uuid>>,
    device_changes_tx: broadcast::Sender<DeviceChangeEvent>,
}

impl MockFsctDriver {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            next_id: AtomicU32::new(1),
            assignments: Mutex::new(HashMap::new()),
            device_changes_tx: tx,
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

    async fn assign_player_to_device(
        &self,
        player_id: ManagedPlayerId,
        device_id: ManagedDeviceId,
    ) -> anyhow::Result<()> {
        self.assignments.lock().unwrap().insert(player_id, device_id);
        Ok(())
    }

    async fn unassign_player_from_device(
        &self,
        player_id: ManagedPlayerId,
        device_id: ManagedDeviceId,
    ) -> anyhow::Result<()> {
        let mut guard = self.assignments.lock().unwrap();
        if guard.get(&player_id) == Some(&device_id) {
            guard.remove(&player_id);
        }
        Ok(())
    }

    async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> anyhow::Result<()> {
        emit_received(serde_json::json!({
            "type": "received",
            "method": "update_player_state",
            "playerId": player_id.get(),
            "state": new_state,
        }));
        Ok(())
    }

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> anyhow::Result<()> {
        emit_received(serde_json::json!({
            "type": "received",
            "method": "update_player_status",
            "playerId": player_id.get(),
            "status": new_status,
        }));
        Ok(())
    }

    async fn update_player_timeline(
        &self,
        player_id: ManagedPlayerId,
        new_timeline: Option<TimelineInfo>,
    ) -> anyhow::Result<()> {
        emit_received(serde_json::json!({
            "type": "received",
            "method": "update_player_timeline",
            "playerId": player_id.get(),
            "timeline": new_timeline,
        }));
        Ok(())
    }

    async fn update_player_metadata(
        &self,
        player_id: ManagedPlayerId,
        metadata_id: FsctTextMetadata,
        new_text: Option<String>,
    ) -> anyhow::Result<()> {
        emit_received(serde_json::json!({
            "type": "received",
            "method": "update_player_metadata",
            "playerId": player_id.get(),
            "metadataId": metadata_id,
            "text": new_text,
        }));
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
        Ok(self.device_changes_tx.subscribe())
    }

    async fn get_timesync(&self) -> anyhow::Result<TimeSync> {
        Ok(TimeSync::sample_now())
    }
}

async fn stdin_command_loop(tx: broadcast::Sender<DeviceChangeEvent>) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let cmd: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("ipc_test_server: invalid stdin command: {e}");
                continue;
            }
        };
        match cmd["type"].as_str() {
            Some("emit_device_changed") => {
                let event_str = cmd["event"].as_str().unwrap_or("");
                let device_id_str = cmd["deviceId"].as_str().unwrap_or("");
                let device_id = match Uuid::parse_str(device_id_str) {
                    Ok(id) => id,
                    Err(e) => {
                        eprintln!("ipc_test_server: invalid deviceId: {e}");
                        continue;
                    }
                };
                let event = match event_str {
                    "added" => DeviceChangeEvent::Added(device_id),
                    "removed" => DeviceChangeEvent::Removed(device_id),
                    _ => {
                        eprintln!("ipc_test_server: unknown event type: {event_str}");
                        continue;
                    }
                };
                let _ = tx.send(event);
            }
            Some(t) => eprintln!("ipc_test_server: unknown command type: {t}"),
            None => eprintln!("ipc_test_server: invalid command (no type field)"),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let endpoint = std::env::args().nth(1).expect("Usage: ipc_test_server <socket-path>");

    let driver = Arc::new(MockFsctDriver::new());
    let tx = driver.device_changes_tx.clone();

    tokio::spawn(stdin_command_loop(tx));

    let mut server = IpcServer::with_socket_path(driver, &endpoint);

    let listener = server.init_listener().await?;
    println!("{}", serde_json::json!({"type": "ready"}));
    let _ = std::io::stdout().flush();

    tokio::select! {
        res = server.accept_listener(listener) => {
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
