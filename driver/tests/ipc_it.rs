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

// Consolidated happy-path IPC integration tests
// Covers: register/unregister, assign/unassign, updates, preferred player and assigned device

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fsct::FsctDriver;
use fsct::definitions::ManagedPlayerId;
use fsct::definitions::{DeviceInfo, FsctStatus, FsctTextMetadata, ManagedDeviceId, TimeSync, TimelineInfo};
use fsct::player_state::{PlayerState, TrackMetadata};
use fsct_client::IpcDriver;
use fsct_driver::IpcServer;
use serde_json::json;

fn test_endpoint() -> String {
    #[cfg(windows)]
    {
        let suffix = format!("{}", uuid::Uuid::new_v4());
        return format!("\\\\.\\pipe\\fsct_host_test_{}", suffix);
    }
    #[cfg(unix)]
    {
        let suffix = format!("{}", uuid::Uuid::new_v4());
        let mut path = std::env::temp_dir();
        path.push(format!("fsct_host_test_{}.sock", suffix));
        return path.to_string_lossy().to_string();
    }
}

// ---------------------------------------------------------------------------
// Raw JSON-RPC client for error-path tests
// ---------------------------------------------------------------------------

mod raw_client {
    #[cfg(unix)]
    use anyhow::Context;
    use fsct_client::rpc::{MAX_LINE_BYTES, RpcRequest, RpcResponse};
    use futures::{SinkExt, StreamExt};
    use serde_json::{Value as JsonValue, json};
    use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

    pub struct RawJsonRpcClient {
        reader: FramedRead<tokio::io::ReadHalf<RawStream>, LinesCodec>,
        writer: FramedWrite<tokio::io::WriteHalf<RawStream>, LinesCodec>,
        next_id: u64,
    }

    #[cfg(unix)]
    type RawStream = tokio::net::UnixStream;
    #[cfg(windows)]
    type RawStream = tokio::net::windows::named_pipe::NamedPipeClient;

    impl RawJsonRpcClient {
        pub async fn connect(endpoint: String) -> anyhow::Result<Self> {
            #[cfg(unix)]
            let stream = tokio::net::UnixStream::connect(&endpoint)
                .await
                .with_context(|| format!("connect to {}", endpoint))?;
            #[cfg(windows)]
            let stream = tokio::net::windows::named_pipe::ClientOptions::new()
                .read(true)
                .write(true)
                .open(&endpoint)?;

            let (r, w) = tokio::io::split(stream);
            Ok(Self {
                reader: FramedRead::new(r, LinesCodec::new_with_max_length(MAX_LINE_BYTES)),
                writer: FramedWrite::new(w, LinesCodec::new_with_max_length(MAX_LINE_BYTES)),
                next_id: 1,
            })
        }

        /// Send an arbitrary raw line (bypasses RpcRequest serialization).
        pub async fn send_raw_line(&mut self, line: &str) -> anyhow::Result<()> {
            self.writer
                .send(line.to_string())
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        }

        /// Read one raw response line from the server, returning None on EOF.
        pub async fn read_response(&mut self) -> Option<anyhow::Result<RpcResponse>> {
            match self.reader.next().await {
                None => None,
                Some(Ok(line)) => {
                    Some(serde_json::from_str::<RpcResponse>(&line).map_err(|e| anyhow::anyhow!("{}", e)))
                }
                Some(Err(e)) => Some(Err(anyhow::anyhow!("{}", e))),
            }
        }

        /// Send a request and return `Ok(result)` or `Err(error_message)`.
        pub async fn request(&mut self, method: &str, params: JsonValue) -> Result<JsonValue, String> {
            let id = self.next_id;
            self.next_id += 1;
            let req = RpcRequest {
                jsonrpc: "2.0".into(),
                id: json!(id),
                method: method.into(),
                params,
            };
            let line = serde_json::to_string(&req).unwrap();
            self.writer.send(line).await.map_err(|e| e.to_string())?;
            let resp_line = self
                .reader
                .next()
                .await
                .ok_or("connection closed".to_string())?
                .map_err(|e| e.to_string())?;
            let resp: RpcResponse = serde_json::from_str(&resp_line).map_err(|e| e.to_string())?;
            if let Some(err) = resp.error {
                Err(err.message)
            } else {
                Ok(resp.result.unwrap_or(JsonValue::Null))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

mod helpers {
    use super::*;
    use fsct_driver::{JoinableTaskHandle, spawn_service};

    async fn start_server_and_connect_common<T, C, Fut>(
        driver: Arc<dyn FsctDriver>,
        connector: C,
    ) -> (T, JoinableTaskHandle)
    where
        C: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        let endpoint = super::test_endpoint();
        let mut server = IpcServer::with_socket_path(driver, endpoint.as_str());
        let server_task = spawn_service(async move |mut s| -> () {
            tokio::select! {
                res = server.serve() => res.unwrap(),
                _ = s.signaled() => (),
            }
            server.shutdown().await;
        });
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(5);
        let client = loop {
            match connector(endpoint.clone()).await {
                Ok(c) => break c,
                Err(e) => {
                    log::debug!("Failed to connect to IPC server: {}", e);
                    if start.elapsed() > timeout {
                        server_task.abort();
                        panic!("Failed to connect to IPC server in time");
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };
        (client, server_task)
    }

    pub async fn start_server_and_connect(driver: Arc<dyn FsctDriver>) -> (IpcDriver, JoinableTaskHandle) {
        start_server_and_connect_common(driver, |endpoint| async move {
            IpcDriver::connect_to_endpoint(endpoint).await
        })
        .await
    }

    pub async fn start_server_and_connect_raw(
        driver: Arc<dyn FsctDriver>,
    ) -> (super::raw_client::RawJsonRpcClient, JoinableTaskHandle) {
        start_server_and_connect_common(driver, |endpoint| async move {
            super::raw_client::RawJsonRpcClient::connect(endpoint).await
        })
        .await
    }

    pub struct FsctDriverMock {
        pub enable_register: bool,
        pub enable_unregister: bool,
        pub enable_assign: bool,
        pub enable_unassign: bool,
        pub enable_update_state: bool,
        pub enable_update_status: bool,
        pub enable_update_timeline: bool,
        pub enable_update_metadata: bool,
        pub enable_get_assigned_device: bool,
        pub enable_get_detected_devices: bool,
        pub enable_subscribe_device_changes: bool,

        pub register_calls: Mutex<Vec<String>>,
        pub unregister_calls: Mutex<Vec<u32>>,
        pub fixed_id: ManagedPlayerId,

        pub assign_calls: Mutex<Vec<(u32, uuid::Uuid)>>,
        pub unassign_calls: Mutex<Vec<(u32, uuid::Uuid)>>,
        pub assigned_device: Mutex<Option<uuid::Uuid>>,
        pub last_assigned_query: Mutex<Vec<u32>>,

        pub state_calls: Mutex<Vec<(u32, PlayerState)>>,
        pub status_calls: Mutex<Vec<(u32, FsctStatus)>>,
        pub timeline_calls: Mutex<Vec<(u32, Option<TimelineInfo>)>>,
        pub metadata_calls: Mutex<Vec<(u32, FsctTextMetadata, Option<String>)>>,

        pub detected_devices: Mutex<Vec<DeviceInfo>>,
        pub device_changes_tx: tokio::sync::broadcast::Sender<fsct::DeviceChangeEvent>,

        // get_timesync always succeeds (the IpcDriver handshake runs it on every connect).
        pub timesync_calls: Mutex<u32>,
        pub timesync_override: Mutex<Option<TimeSync>>,
    }

    impl FsctDriverMock {
        pub fn new() -> Self {
            let (tx, _rx) = tokio::sync::broadcast::channel(16);
            Self {
                enable_register: false,
                enable_unregister: false,
                enable_assign: false,
                enable_unassign: false,
                enable_update_state: false,
                enable_update_status: false,
                enable_update_timeline: false,
                enable_update_metadata: false,
                enable_get_assigned_device: false,
                enable_get_detected_devices: false,
                enable_subscribe_device_changes: false,
                register_calls: Mutex::new(Vec::new()),
                unregister_calls: Mutex::new(Vec::new()),
                fixed_id: std::num::NonZeroU32::new(1).unwrap(),
                assign_calls: Mutex::new(Vec::new()),
                unassign_calls: Mutex::new(Vec::new()),
                assigned_device: Mutex::new(None),
                last_assigned_query: Mutex::new(Vec::new()),
                state_calls: Mutex::new(Vec::new()),
                status_calls: Mutex::new(Vec::new()),
                timeline_calls: Mutex::new(Vec::new()),
                metadata_calls: Mutex::new(Vec::new()),
                detected_devices: Mutex::new(Vec::new()),
                device_changes_tx: tx,
                timesync_calls: Mutex::new(0),
                timesync_override: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl FsctDriver for FsctDriverMock {
        async fn register_player(&self, self_id: String) -> anyhow::Result<ManagedPlayerId> {
            if !self.enable_register {
                return Err(anyhow::anyhow!("not used"));
            }
            self.register_calls.lock().unwrap().push(self_id);
            Ok(self.fixed_id)
        }
        async fn unregister_player(&self, player_id: ManagedPlayerId) -> anyhow::Result<()> {
            if !self.enable_unregister {
                return Err(anyhow::anyhow!("not used"));
            }
            self.unregister_calls.lock().unwrap().push(player_id.get());
            Ok(())
        }
        async fn assign_player_to_device(
            &self,
            player_id: ManagedPlayerId,
            device_id: ManagedDeviceId,
        ) -> anyhow::Result<()> {
            if !self.enable_assign {
                return Err(anyhow::anyhow!("not used"));
            }
            self.assign_calls.lock().unwrap().push((player_id.get(), device_id));
            Ok(())
        }
        async fn unassign_player_from_device(
            &self,
            player_id: ManagedPlayerId,
            device_id: ManagedDeviceId,
        ) -> anyhow::Result<()> {
            if !self.enable_unassign {
                return Err(anyhow::anyhow!("not used"));
            }
            self.unassign_calls.lock().unwrap().push((player_id.get(), device_id));
            Ok(())
        }
        async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> anyhow::Result<()> {
            if !self.enable_update_state {
                return Err(anyhow::anyhow!("not used"));
            }
            self.state_calls.lock().unwrap().push((player_id.get(), new_state));
            Ok(())
        }
        async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> anyhow::Result<()> {
            if !self.enable_update_status {
                return Err(anyhow::anyhow!("not used"));
            }
            self.status_calls.lock().unwrap().push((player_id.get(), new_status));
            Ok(())
        }
        async fn update_player_timeline(
            &self,
            player_id: ManagedPlayerId,
            new_timeline: Option<TimelineInfo>,
        ) -> anyhow::Result<()> {
            if !self.enable_update_timeline {
                return Err(anyhow::anyhow!("not used"));
            }
            self.timeline_calls
                .lock()
                .unwrap()
                .push((player_id.get(), new_timeline));
            Ok(())
        }
        async fn update_player_metadata(
            &self,
            player_id: ManagedPlayerId,
            metadata_id: FsctTextMetadata,
            new_text: Option<String>,
        ) -> anyhow::Result<()> {
            if !self.enable_update_metadata {
                return Err(anyhow::anyhow!("not used"));
            }
            self.metadata_calls
                .lock()
                .unwrap()
                .push((player_id.get(), metadata_id, new_text));
            Ok(())
        }
        async fn get_player_assigned_device(
            &self,
            player_id: ManagedPlayerId,
        ) -> anyhow::Result<Option<ManagedDeviceId>> {
            if !self.enable_get_assigned_device {
                return Err(anyhow::anyhow!("not used"));
            }
            self.last_assigned_query.lock().unwrap().push(player_id.get());
            Ok(*self.assigned_device.lock().unwrap())
        }
        async fn get_detected_devices(&self) -> anyhow::Result<Vec<ManagedDeviceId>> {
            if !self.enable_get_detected_devices {
                return Err(anyhow::anyhow!("not used"));
            }
            Ok(self.detected_devices.lock().unwrap().iter().map(|d| d.id).collect())
        }
        async fn subscribe_device_changes(
            &self,
        ) -> anyhow::Result<tokio::sync::broadcast::Receiver<fsct::DeviceChangeEvent>> {
            if !self.enable_subscribe_device_changes {
                return Err(anyhow::anyhow!("not used"));
            }
            Ok(self.device_changes_tx.subscribe())
        }
        async fn get_device_info(&self, device_id: ManagedDeviceId) -> anyhow::Result<DeviceInfo> {
            let list = self.detected_devices.lock().unwrap();
            list.iter()
                .find(|d| d.id == device_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("not found"))
        }
        async fn get_timesync(&self) -> anyhow::Result<TimeSync> {
            *self.timesync_calls.lock().unwrap() += 1;
            Ok(self
                .timesync_override
                .lock()
                .unwrap()
                .unwrap_or_else(TimeSync::sample_now))
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_register_and_unregister_player() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let fixed_id = std::num::NonZeroU32::new(123).unwrap();
    let mut um = helpers::FsctDriverMock::new();
    um.enable_register = true;
    um.enable_unregister = true;
    um.fixed_id = fixed_id;
    let mock = Arc::new(um);

    let (client, server_task) = helpers::start_server_and_connect(mock.clone()).await;

    let self_id = "test_player_self_id".to_string();
    let player_id = client.register_player(self_id.clone()).await?;
    assert_eq!(player_id, fixed_id);
    {
        let calls = mock.register_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], self_id);
    }

    client.unregister_player(player_id).await?;
    {
        let calls = mock.unregister_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], fixed_id.get());
    }

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_assign_and_unassign_player() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut um = helpers::FsctDriverMock::new();
    um.enable_assign = true;
    um.enable_unassign = true;
    um.enable_get_assigned_device = true;
    um.enable_register = true;
    um.fixed_id = std::num::NonZeroU32::new(42).unwrap();
    let mock = Arc::new(um);
    let (client, server_task) = helpers::start_server_and_connect(mock.clone()).await;

    let _ = client.register_player("p42".to_string()).await?;
    let player_id = std::num::NonZeroU32::new(42).unwrap();
    let device_id = uuid::Uuid::new_v4();

    client.assign_player_to_device(player_id, device_id).await?;
    {
        let calls = mock.assign_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, device_id);
    }

    client.unassign_player_from_device(player_id, device_id).await?;
    {
        let calls = mock.unassign_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, device_id);
    }

    let device2 = uuid::Uuid::new_v4();
    *mock.assigned_device.lock().unwrap() = Some(device2);
    let dev = client.get_player_assigned_device(player_id).await?;
    assert_eq!(dev, Some(device2));
    let queries = mock.last_assigned_query.lock().unwrap().clone();
    assert_eq!(queries.last().copied(), Some(player_id.get()));

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_update_methods() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut um = helpers::FsctDriverMock::new();
    um.enable_update_status = true;
    um.enable_update_timeline = true;
    um.enable_update_metadata = true;
    um.enable_update_state = true;
    um.enable_register = true;
    um.fixed_id = std::num::NonZeroU32::new(7).unwrap();
    let mock = Arc::new(um);

    let (client, server_task) = helpers::start_server_and_connect(mock.clone()).await;

    let player_id = client.register_player("p7".to_string()).await?;

    // status
    client.update_player_status(player_id, FsctStatus::Playing).await?;
    {
        let calls = mock.status_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, FsctStatus::Playing);
    }

    // timeline
    let timeline = TimelineInfo {
        position: Duration::from_millis(12345),
        update_time: Instant::now(),
        duration: Duration::from_millis(54321),
        rate: 1.0,
    };
    client.update_player_timeline(player_id, Some(timeline.clone())).await?;
    {
        let calls = mock.timeline_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert!(calls[0].1.is_some());
        let got = calls[0].1.as_ref().unwrap();
        assert_eq!(got.position, timeline.position);
        assert_eq!(got.duration, timeline.duration);
        assert!((got.rate - timeline.rate).abs() < 1e-9);
        // The monotonic anchor must survive the client->driver frame conversion. In-process the
        // two share a process EPOCH (handshake offset ~0), so it round-trips to within tolerance.
        let drift = got.update_time.saturating_duration_since(timeline.update_time)
            + timeline.update_time.saturating_duration_since(got.update_time);
        assert!(
            drift < Duration::from_millis(50),
            "update_time anchor drifted by {:?}",
            drift
        );
    }

    // metadata
    client
        .update_player_metadata(player_id, FsctTextMetadata::CurrentTitle, Some("Track X".to_string()))
        .await?;
    {
        let calls = mock.metadata_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, FsctTextMetadata::CurrentTitle);
        assert_eq!(calls[0].2.as_deref(), Some("Track X"));
    }

    // full state
    let mut state = PlayerState::default();
    state.status = FsctStatus::Paused;
    state.timeline = None;
    state.texts = TrackMetadata {
        title: Some("T".into()),
        artist: None,
        album: Some("Alb".into()),
        genre: None,
    };
    client.update_player_state(player_id, state.clone()).await?;
    {
        let calls = mock.state_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1.status, state.status);
        assert_eq!(calls[0].1.timeline, state.timeline);
        assert_eq!(calls[0].1.texts.title, state.texts.title);
        assert_eq!(calls[0].1.texts.album, state.texts.album);
    }

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_get_protocol_version() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let driver = Arc::new(helpers::FsctDriverMock::new());
    let (client, server_task) = helpers::start_server_and_connect(driver).await;

    let ver = client.get_protocol_version().await?;
    assert_eq!(ver, fsct::FSCT_PROTOCOL_VERSION);

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_parsing_errors_are_returned_to_client() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let driver = Arc::new(helpers::FsctDriverMock::new());
    let (mut client, server_task) = helpers::start_server_and_connect_raw(driver).await;

    // ---- update_player_state ----
    // missing player_id
    assert!(client.request("update_player_state", json!({})).await.is_err());
    // missing state
    assert!(
        client
            .request("update_player_state", json!({"player_id": 1}))
            .await
            .is_err()
    );
    // extra params (actually not an error in JSON, but invalid player_id types)
    assert!(
        client
            .request("update_player_state", json!({"player_id": "1", "state": {}}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("update_player_state", json!({"player_id": 0, "state": {}}))
            .await
            .is_err()
    );

    // wrong key name in state
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 1,
                    "state": {"statsu": "playing", "texts": {}, "timeline": null}
                })
            )
            .await
            .is_err()
    );

    // unknown texts sub-key
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 2,
                    "state": {"status": "playing", "texts": {"titl": "X"}, "timeline": null}
                })
            )
            .await
            .is_err()
    );

    // texts wrong value type
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 3,
                    "state": {"status": "playing", "texts": {"title": 123}, "timeline": null}
                })
            )
            .await
            .is_err()
    );

    // timeline wrong type (not null and not object)
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 4,
                    "state": {"timeline": 1}
                })
            )
            .await
            .is_err()
    );

    // status wrong type (integer instead of string)
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 5,
                    "state": {"status": 1}
                })
            )
            .await
            .is_err()
    );

    // status invalid string
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 6,
                    "state": {"status": "invalid_status_value"}
                })
            )
            .await
            .is_err()
    );

    // timeline missing required fields
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 7,
                    "state": {"timeline": {"position_ms": 1}}
                })
            )
            .await
            .is_err()
    );

    // texts not an object
    assert!(
        client
            .request(
                "update_player_state",
                json!({
                    "player_id": 8,
                    "state": {"texts": 5}
                })
            )
            .await
            .is_err()
    );

    // state not an object
    assert!(
        client
            .request("update_player_state", json!({"player_id": 9, "state": 1}))
            .await
            .is_err()
    );

    // ---- update_player_status ----
    assert!(client.request("update_player_status", json!({})).await.is_err());
    assert!(
        client
            .request("update_player_status", json!({"player_id": 1}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("update_player_status", json!({"player_id": "1", "status": "playing"}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("update_player_status", json!({"player_id": 0, "status": "playing"}))
            .await
            .is_err()
    );
    // status wrong type
    assert!(
        client
            .request("update_player_status", json!({"player_id": 10, "status": 1}))
            .await
            .is_err()
    );
    // status invalid string
    assert!(
        client
            .request("update_player_status", json!({"player_id": 10, "status": "invalid"}))
            .await
            .is_err()
    );

    // ---- update_player_timeline ----
    assert!(client.request("update_player_timeline", json!({})).await.is_err());
    assert!(
        client
            .request("update_player_timeline", json!({"player_id": 1}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("update_player_timeline", json!({"player_id": "1", "timeline": null}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("update_player_timeline", json!({"player_id": 0, "timeline": null}))
            .await
            .is_err()
    );
    // timeline wrong type
    assert!(
        client
            .request("update_player_timeline", json!({"player_id": 11, "timeline": 5}))
            .await
            .is_err()
    );
    // timeline unknown key
    assert!(
        client
            .request(
                "update_player_timeline",
                json!({
                    "player_id": 11,
                    "timeline": {"pos": 1}
                })
            )
            .await
            .is_err()
    );
    // timeline missing position_ms
    assert!(
        client
            .request(
                "update_player_timeline",
                json!({
                    "player_id": 12,
                    "timeline": {"update_mono_ms": 0, "duration_ms": 2000, "rate": 1.0}
                })
            )
            .await
            .is_err()
    );
    // timeline missing update_mono_ms
    assert!(
        client
            .request(
                "update_player_timeline",
                json!({
                    "player_id": 13,
                    "timeline": {"position_ms": 1000, "duration_ms": 2000, "rate": 1.0}
                })
            )
            .await
            .is_err()
    );
    // timeline missing duration_ms
    assert!(
        client
            .request(
                "update_player_timeline",
                json!({
                    "player_id": 14,
                    "timeline": {"position_ms": 1000, "update_mono_ms": 0, "rate": 1.0}
                })
            )
            .await
            .is_err()
    );
    // timeline missing rate
    assert!(
        client
            .request(
                "update_player_timeline",
                json!({
                    "player_id": 15,
                    "timeline": {"position_ms": 1000, "update_mono_ms": 0, "duration_ms": 2000}
                })
            )
            .await
            .is_err()
    );
    // timeline wrong field types
    assert!(
        client
            .request(
                "update_player_timeline",
                json!({
                    "player_id": 16,
                    "timeline": {"position_ms": "1000", "update_mono_ms": "0", "duration_ms": "2000", "rate": "1.0"}
                })
            )
            .await
            .is_err()
    );

    // ---- update_player_metadata ----
    assert!(client.request("update_player_metadata", json!({})).await.is_err());
    assert!(
        client
            .request("update_player_metadata", json!({"player_id": 1}))
            .await
            .is_err()
    );
    assert!(
        client
            .request(
                "update_player_metadata",
                json!({"player_id": 1, "metadata_id": "current_title"})
            )
            .await
            .is_err()
    );
    // invalid player_id
    assert!(
        client
            .request(
                "update_player_metadata",
                json!({"player_id": 0, "metadata_id": "current_title", "text": null})
            )
            .await
            .is_err()
    );
    assert!(
        client
            .request(
                "update_player_metadata",
                json!({"player_id": "1", "metadata_id": "current_title", "text": null})
            )
            .await
            .is_err()
    );
    // invalid metadata_id
    assert!(
        client
            .request(
                "update_player_metadata",
                json!({"player_id": 1, "metadata_id": "invalid_meta", "text": null})
            )
            .await
            .is_err()
    );
    // metadata_id wrong type
    assert!(
        client
            .request(
                "update_player_metadata",
                json!({"player_id": 1, "metadata_id": 1, "text": null})
            )
            .await
            .is_err()
    );
    // text wrong type
    assert!(
        client
            .request(
                "update_player_metadata",
                json!({"player_id": 1, "metadata_id": "current_title", "text": 123})
            )
            .await
            .is_err()
    );

    // ---- set_preferred_player (unknown method → error) ----
    assert!(client.request("set_preferred_player", json!({})).await.is_err());
    assert!(
        client
            .request("set_preferred_player", json!({"player_id": 0}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("set_preferred_player", json!({"player_id": "1"}))
            .await
            .is_err()
    );

    // ---- get_preferred_player (unknown method → error) ----
    assert!(
        client
            .request("get_preferred_player", json!({"player_id": 1}))
            .await
            .is_err()
    );

    // ---- get_player_assigned_device ----
    assert!(client.request("get_player_assigned_device", json!({})).await.is_err());
    assert!(
        client
            .request("get_player_assigned_device", json!({"player_id": "1"}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("get_player_assigned_device", json!({"player_id": null}))
            .await
            .is_err()
    );
    assert!(
        client
            .request("get_player_assigned_device", json!({"player_id": 0}))
            .await
            .is_err()
    );

    // nil/null player_id
    assert!(
        client
            .request("update_player_status", json!({"player_id": null, "status": "playing"}))
            .await
            .is_err()
    );

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_player_id_scope_validation_errors() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut um = helpers::FsctDriverMock::new();
    um.enable_register = true;
    um.fixed_id = std::num::NonZeroU32::new(50).unwrap();
    let mock = std::sync::Arc::new(um);

    let (client, server_task) = helpers::start_server_and_connect(mock.clone()).await;

    let _registered = client.register_player("p50".to_string()).await?;
    let bad_pid = std::num::NonZeroU32::new(999).unwrap();

    assert!(client.unregister_player(bad_pid).await.is_err());
    let dev = uuid::Uuid::new_v4();
    assert!(client.assign_player_to_device(bad_pid, dev).await.is_err());
    assert!(client.unassign_player_from_device(bad_pid, dev).await.is_err());
    assert!(client.update_player_status(bad_pid, FsctStatus::Playing).await.is_err());
    let tl = TimelineInfo {
        position: std::time::Duration::from_millis(1),
        update_time: std::time::Instant::now(),
        duration: std::time::Duration::from_millis(2),
        rate: 1.0,
    };
    assert!(client.update_player_timeline(bad_pid, Some(tl)).await.is_err());
    assert!(
        client
            .update_player_metadata(bad_pid, FsctTextMetadata::CurrentTitle, Some("X".into()))
            .await
            .is_err()
    );
    let mut st = PlayerState::default();
    st.status = FsctStatus::Paused;
    assert!(client.update_player_state(bad_pid, st).await.is_err());
    assert!(client.get_player_assigned_device(bad_pid).await.is_err());

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_auto_unregister_on_disconnect() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut um = helpers::FsctDriverMock::new();
    um.enable_register = true;
    um.enable_unregister = true;
    um.fixed_id = std::num::NonZeroU32::new(201).unwrap();
    let mock = std::sync::Arc::new(um);

    let server_task;
    {
        let (client_local, st) = helpers::start_server_and_connect(mock.clone()).await;
        server_task = st;
        let p1 = client_local.register_player("p201".to_string()).await?;
        assert_eq!(p1.get(), 201);
    }

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let calls = mock.unregister_calls.lock().unwrap().clone();
    assert_eq!(calls, vec![201]);

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_get_detected_devices() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let device1 = DeviceInfo {
        id: uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap(),
        name: Some("Test Device 1".to_string()),
        manufacturer: Some("Test Manufacturer".to_string()),
        vendor_id: 0x1234,
        product_id: 0x5678,
        serial_number: Some("SN001".to_string()),
    };
    let device2 = DeviceInfo {
        id: uuid::Uuid::parse_str("87654321-4321-8765-4321-876543218765").unwrap(),
        name: None,
        manufacturer: Some("Another Manufacturer".to_string()),
        vendor_id: 0x8765,
        product_id: 0x4321,
        serial_number: None,
    };

    let mut um = helpers::FsctDriverMock::new();
    um.enable_get_detected_devices = true;
    um.detected_devices = Mutex::new(vec![device1.clone(), device2.clone()]);
    let mock = Arc::new(um);

    let (client, server_task) = helpers::start_server_and_connect(mock.clone()).await;

    let device_ids = client.get_detected_devices().await?;
    assert_eq!(device_ids.len(), 2);

    let d1 = client.get_device_info(device_ids[0]).await?;
    assert_eq!(d1, device1);
    let d2 = client.get_device_info(device_ids[1]).await?;
    assert_eq!(d2, device2);

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_subscribe_device_changes() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut um = helpers::FsctDriverMock::new();
    um.enable_subscribe_device_changes = true;
    let mock = Arc::new(um);

    let (client, server_task) = helpers::start_server_and_connect(mock.clone()).await;

    let mut rx = client.subscribe_device_changes().await?;

    let dev = uuid::Uuid::new_v4();
    let _ = mock.device_changes_tx.send(fsct::DeviceChangeEvent::Added(dev));

    let evt = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await??;
    match evt {
        fsct::DeviceChangeEvent::Added(id) => assert_eq!(id, dev),
        _ => panic!("expected Added"),
    }

    let _ = mock.device_changes_tx.send(fsct::DeviceChangeEvent::Removed(dev));
    let evt2 = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await??;
    match evt2 {
        fsct::DeviceChangeEvent::Removed(id) => assert_eq!(id, dev),
        _ => panic!("expected Removed"),
    }

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_driver_errors_become_application_errors() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // register is enabled; all other methods keep their default enable=false → driver returns Err
    let mut um = helpers::FsctDriverMock::new();
    um.enable_register = true;
    um.fixed_id = std::num::NonZeroU32::new(77).unwrap();
    let mock = Arc::new(um);

    let (mut client, server_task) = helpers::start_server_and_connect_raw(mock.clone()).await;

    let pid = client
        .request("register_player", json!({"self_id": "p77"}))
        .await
        .expect("register_player should succeed");
    let pid = pid.as_u64().unwrap();

    // unregister_player: scope check passes (player is registered), but driver returns Err
    let err = client.request("unregister_player", json!({"player_id": pid})).await;
    assert!(err.is_err(), "driver error should be returned to client");

    // update_player_status: driver returns Err → ERR_APPLICATION
    let err2 = client
        .request("update_player_status", json!({"player_id": pid, "status": "playing"}))
        .await;
    assert!(err2.is_err(), "driver error should be returned to client");

    // assign_player_to_device: driver returns Err → ERR_APPLICATION
    let device_id = uuid::Uuid::new_v4().to_string();
    let err3 = client
        .request(
            "assign_player_to_device",
            json!({"player_id": pid, "device_id": device_id}),
        )
        .await;
    assert!(err3.is_err(), "driver error should be returned to client");

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_malformed_jsonrpc_structure() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let driver = Arc::new(helpers::FsctDriverMock::new());
    let (mut client, server_task) = helpers::start_server_and_connect_raw(driver).await;

    // Empty object — missing jsonrpc, id, method
    client.send_raw_line("{}").await?;
    let resp = client
        .read_response()
        .await
        .expect("server should respond to malformed request")
        .expect("response should deserialize");
    assert!(resp.error.is_some(), "expected error for missing fields");
    assert_eq!(
        resp.error.unwrap().code,
        fsct_client::rpc::ERR_PARSE_ERROR,
        "expected ERR_PARSE_ERROR code"
    );

    // Missing id field only
    client
        .send_raw_line(r#"{"jsonrpc":"2.0","method":"get_protocol_version"}"#)
        .await?;
    let resp2 = client
        .read_response()
        .await
        .expect("server should respond to missing-id request")
        .expect("response should deserialize");
    assert!(resp2.error.is_some(), "expected error for missing id");
    assert_eq!(
        resp2.error.unwrap().code,
        fsct_client::rpc::ERR_PARSE_ERROR,
        "expected ERR_PARSE_ERROR code"
    );

    // Connection must survive parse errors — a valid request should still work
    let ok = client.request("get_protocol_version", json!({})).await;
    assert!(ok.is_ok(), "connection should remain alive after parse errors");

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_oversized_line_closes_connection() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let driver = Arc::new(helpers::FsctDriverMock::new());
    let (mut client, server_task) = helpers::start_server_and_connect_raw(driver).await;

    // Send a line larger than MAX_LINE_BYTES (1 MiB); server's LinesCodec returns an error and closes the connection
    let oversized = "x".repeat(fsct_client::rpc::MAX_LINE_BYTES + 1);
    client.send_raw_line(&oversized).await?;

    // Server closes the connection — next read should return None (EOF)
    let resp = tokio::time::timeout(Duration::from_secs(2), async { client.read_response().await })
        .await
        .expect("timed out waiting for server to close connection");
    assert!(resp.is_none(), "server should close connection after oversized line");

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_get_timesync_round_trips_wall_and_mono() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mock = Arc::new(helpers::FsctDriverMock::new());
    // Pin the driver's reply so we can assert the exact values cross the wire unchanged.
    *mock.timesync_override.lock().unwrap() = Some(TimeSync {
        wall_ms: 1_700_000_000_000,
        mono_ms: 42_000,
    });

    // Use the raw client so no handshake runs; we drive get_timesync explicitly.
    let (mut client, server_task) = helpers::start_server_and_connect_raw(mock.clone()).await;

    let result = client
        .request("get_timesync", json!({}))
        .await
        .expect("get_timesync should succeed");
    assert_eq!(result["wall_ms"].as_u64(), Some(1_700_000_000_000));
    assert_eq!(result["mono_ms"].as_u64(), Some(42_000));
    assert_eq!(*mock.timesync_calls.lock().unwrap(), 1);

    server_task.shutdown().await?;
    Ok(())
}
