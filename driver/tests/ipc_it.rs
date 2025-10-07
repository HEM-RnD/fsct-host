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
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use msgpack_rpc::Value;
use fsct_client::IpcDriver;
use fsct_driver::IpcServer;
use fsct::FsctDriver;
use fsct::definitions::ManagedPlayerId;
use fsct::definitions::{DeviceInfo, FsctStatus, FsctTextMetadata, ManagedDeviceId, TimelineInfo};
use fsct::player_state::{PlayerState, TrackMetadata};

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

// Shared test helpers for mocks: unified configurable mock
mod helpers {
    use anyhow::Context;
    use fsct_driver::{spawn_service, JoinableTaskHandle};
    use super::*;

    // Common helper used by both connect helpers: spawns server and retries connection via provided connector
    async fn start_server_and_connect_common<T, C, Fut>(
        driver: Arc<dyn FsctDriver>,
        connector: C,
    ) -> (T, JoinableTaskHandle)
    where
        C: Fn(String) -> Fut,
        Fut: std::future::Future<Output=Result<T, anyhow::Error>>,
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
                    log::error!("Failed to connect to IPC server: {}", e);
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

    // Unified helper to start server and connect client with retry
    pub async fn start_server_and_connect(driver: Arc<dyn FsctDriver>) -> (IpcDriver, JoinableTaskHandle) {
        start_server_and_connect_common(driver, |endpoint: String| async move {
            IpcDriver::connect_to_endpoint(endpoint).await
        }).await
    }

    // Unified helper to start server and connect a raw msgpack-rpc Client (for parsing error tests)
    pub async fn start_server_and_connect_raw(driver: Arc<dyn FsctDriver>) -> (msgpack_rpc::Client, JoinableTaskHandle) {
        use parity_tokio_ipc::Endpoint;
        use tokio_util::compat::TokioAsyncReadCompatExt;
        start_server_and_connect_common(driver, |endpoint: String| async move {
            Endpoint::connect(endpoint).await.map(|stream| msgpack_rpc::Client::new(stream.compat())).with_context(|| "Failed to connect to IPC server at {}")
        }).await
    }

    pub struct FsctDriverMock {
        // enable flags per method
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

        // register/unregister captures
        pub register_calls: Mutex<Vec<String>>,
        pub unregister_calls: Mutex<Vec<u32>>,
        pub fixed_id: ManagedPlayerId,

        // assign/unassign captures
        pub assign_calls: Mutex<Vec<(u32, uuid::Uuid)>>,
        pub unassign_calls: Mutex<Vec<(u32, uuid::Uuid)>>,
        pub assigned_device: Mutex<Option<uuid::Uuid>>, // response storage for getter
        pub last_assigned_query: Mutex<Vec<u32>>,

        // updates captures
        pub state_calls: Mutex<Vec<(u32, PlayerState)>>,
        pub status_calls: Mutex<Vec<(u32, FsctStatus)>>,
        pub timeline_calls: Mutex<Vec<(u32, Option<TimelineInfo>)>>,
        pub metadata_calls: Mutex<Vec<(u32, FsctTextMetadata, Option<String>)>>,

        // devices
        pub detected_devices: Mutex<Vec<DeviceInfo>>,
    }

    impl FsctDriverMock {
        pub fn new() -> Self {
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
            }
        }
    }

    #[async_trait]
    impl FsctDriver for FsctDriverMock {
        async fn register_player(&self, self_id: String) -> anyhow::Result<ManagedPlayerId, anyhow::Error> {
            if !self.enable_register { return Err(anyhow::anyhow!("not used")); }
            self.register_calls.lock().unwrap().push(self_id);
            Ok(self.fixed_id)
        }
        async fn unregister_player(&self, player_id: ManagedPlayerId) -> anyhow::Result<(), anyhow::Error> {
            if !self.enable_unregister { return Err(anyhow::anyhow!("not used")); }
            self.unregister_calls.lock().unwrap().push(player_id.get());
            Ok(())
        }
        async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
            if !self.enable_assign { return Err(anyhow::anyhow!("not used")); }
            self.assign_calls.lock().unwrap().push((player_id.get(), device_id));
            Ok(())
        }
        async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
            if !self.enable_unassign { return Err(anyhow::anyhow!("not used")); }
            self.unassign_calls.lock().unwrap().push((player_id.get(), device_id));
            Ok(())
        }
        async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> anyhow::Result<(), anyhow::Error> {
            if !self.enable_update_state { return Err(anyhow::anyhow!("not used")); }
            self.state_calls.lock().unwrap().push((player_id.get(), new_state));
            Ok(())
        }
        async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> anyhow::Result<(), anyhow::Error> {
            if !self.enable_update_status { return Err(anyhow::anyhow!("not used")); }
            self.status_calls.lock().unwrap().push((player_id.get(), new_status));
            Ok(())
        }
        async fn update_player_timeline(&self, player_id: ManagedPlayerId, new_timeline: Option<TimelineInfo>) -> anyhow::Result<(), anyhow::Error> {
            if !self.enable_update_timeline { return Err(anyhow::anyhow!("not used")); }
            self.timeline_calls.lock().unwrap().push((player_id.get(), new_timeline));
            Ok(())
        }
        async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: Option<String>) -> anyhow::Result<(), anyhow::Error> {
            if !self.enable_update_metadata { return Err(anyhow::anyhow!("not used")); }
            self.metadata_calls.lock().unwrap().push((player_id.get(), metadata_id, new_text));
            Ok(())
        }
        async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>, anyhow::Error> {
            if !self.enable_get_assigned_device { return Err(anyhow::anyhow!("not used")); }
            self.last_assigned_query.lock().unwrap().push(player_id.get());
            Ok(*self.assigned_device.lock().unwrap())
        }
        async fn get_detected_devices(&self) -> anyhow::Result<Vec<DeviceInfo>, anyhow::Error> {
            if !self.enable_get_detected_devices { return Err(anyhow::anyhow!("not used")); }
            Ok(self.detected_devices.lock().unwrap().clone())
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

    // register player for this connection (required by server validation)
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
    let q = player_id;
    let dev = client.get_player_assigned_device(q).await?;
    assert_eq!(dev, Some(device2));
    let queries = mock.last_assigned_query.lock().unwrap().clone();
    assert_eq!(queries.last().copied(), Some(q.get()));

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

    // register player for this connection (required by server validation)
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
    let timeline = TimelineInfo { position: Duration::from_millis(12345), update_time: SystemTime::now(), duration: Duration::from_millis(54321), rate: 1.0 };
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
    }

    // metadata
    client.update_player_metadata(player_id, FsctTextMetadata::CurrentTitle, Some("Track X".to_string())).await?;
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
    state.texts = TrackMetadata { title: Some("T".into()), artist: None, album: Some("Alb".into()), genre: None };
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
    let (client, server_task) = helpers::start_server_and_connect_raw(driver).await;

    // ---- update_player_state ----
    // wrong param counts
    assert!(client.request("update_player_state", &[]).await.is_err());
    assert!(client.request("update_player_state", &[Value::from(1u64)]).await.is_err());
    assert!(client.request("update_player_state", &[Value::from(1u64), Value::Map(vec![]), Value::Nil]).await.is_err());

    // invalid player_id types
    assert!(client.request("update_player_state", &[Value::from("1"), Value::Map(vec![])]).await.is_err());
    assert!(client.request("update_player_state", &[Value::from(0u64), Value::Map(vec![])]).await.is_err());

    // wrong key name in state map
    let bad_state = Value::Map(vec![
        (Value::from("statsu"), Value::from(1u64)),
        (Value::from("texts"), Value::Map(vec![])),
        (Value::from("timeline"), Value::Nil),
    ]);
    assert!(client.request("update_player_state", &[Value::from(1u64), bad_state]).await.is_err());

    // unknown texts sub-key
    let bad_state2 = Value::Map(vec![
        (Value::from("status"), Value::from(1u64)),
        (Value::from("texts"), Value::Map(vec![(Value::from("titl"), Value::from("X"))])),
        (Value::from("timeline"), Value::Nil),
    ]);
    assert!(client.request("update_player_state", &[Value::from(2u64), bad_state2]).await.is_err());

    // texts wrong value type
    let bad_state3 = Value::Map(vec![
        (Value::from("status"), Value::from(1u64)),
        (Value::from("texts"), Value::Map(vec![(Value::from("title"), Value::from(123u64))])),
        (Value::from("timeline"), Value::Nil),
    ]);
    assert!(client.request("update_player_state", &[Value::from(3u64), bad_state3]).await.is_err());

    // timeline present but not a map
    let bad_state4 = Value::Map(vec![(Value::from("timeline"), Value::from(1u64))]);
    assert!(client.request("update_player_state", &[Value::from(4u64), bad_state4]).await.is_err());

    // status wrong type
    let bad_state5 = Value::Map(vec![(Value::from("status"), Value::from("x"))]);
    assert!(client.request("update_player_state", &[Value::from(5u64), bad_state5]).await.is_err());

    // status invalid code
    let bad_state6 = Value::Map(vec![(Value::from("status"), Value::from(255u64))]);
    assert!(client.request("update_player_state", &[Value::from(6u64), bad_state6]).await.is_err());

    // timeline inside state missing fields
    let bad_state7 = Value::Map(vec![(Value::from("timeline"), Value::Map(vec![(Value::from("position_ms"), Value::from(1u64))]))]);
    assert!(client.request("update_player_state", &[Value::from(7u64), bad_state7]).await.is_err());

    // texts not a map
    let bad_state8 = Value::Map(vec![(Value::from("texts"), Value::from(5u64))]);
    assert!(client.request("update_player_state", &[Value::from(8u64), bad_state8]).await.is_err());

    // non-map state
    assert!(client.request("update_player_state", &[Value::from(9u64), Value::from(1u64)]).await.is_err());

    // ---- update_player_status ----
    // wrong param counts
    assert!(client.request("update_player_status", &[]).await.is_err());
    assert!(client.request("update_player_status", &[Value::from(1u64)]).await.is_err());
    assert!(client.request("update_player_status", &[Value::from(1u64), Value::from(1u64), Value::Nil]).await.is_err());

    // invalid player_id
    assert!(client.request("update_player_status", &[Value::from("1"), Value::from(1u64)]).await.is_err());
    assert!(client.request("update_player_status", &[Value::from(0u64), Value::from(1u64)]).await.is_err());

    // wrong type for status
    assert!(client.request("update_player_status", &[Value::from(10u64), Value::from("1")]).await.is_err());
    // out of range status code
    assert!(client.request("update_player_status", &[Value::from(10u64), Value::from(255u64)]).await.is_err());

    // ---- update_player_timeline ----
    // wrong param counts
    assert!(client.request("update_player_timeline", &[]).await.is_err());
    assert!(client.request("update_player_timeline", &[Value::from(1u64)]).await.is_err());
    assert!(client.request("update_player_timeline", &[Value::from(1u64), Value::Map(vec![]), Value::Nil]).await.is_err());

    // invalid player_id
    assert!(client.request("update_player_timeline", &[Value::from("1"), Value::Nil]).await.is_err());
    assert!(client.request("update_player_timeline", &[Value::from(0u64), Value::Nil]).await.is_err());

    // timeline wrong type (int)
    assert!(client.request("update_player_timeline", &[Value::from(11u64), Value::from(5u64)]).await.is_err());

    // timeline unknown key
    let bad_timeline2 = Value::Map(vec![(Value::from("pos"), Value::from(1u64))]);
    assert!(client.request("update_player_timeline", &[Value::from(11u64), bad_timeline2]).await.is_err());

    // timeline missing each required field
    let t_missing_pos = Value::Map(vec![
        (Value::from("update_unix_ms"), Value::from(0i64)),
        (Value::from("duration_ms"), Value::from(2000u64)),
        (Value::from("rate"), Value::from(1.0f64)),
    ]);
    assert!(client.request("update_player_timeline", &[Value::from(12u64), t_missing_pos]).await.is_err());

    let t_missing_update = Value::Map(vec![
        (Value::from("position_ms"), Value::from(1000u64)),
        (Value::from("duration_ms"), Value::from(2000u64)),
        (Value::from("rate"), Value::from(1.0f64)),
    ]);
    assert!(client.request("update_player_timeline", &[Value::from(13u64), t_missing_update]).await.is_err());

    let t_missing_duration = Value::Map(vec![
        (Value::from("position_ms"), Value::from(1000u64)),
        (Value::from("update_unix_ms"), Value::from(0i64)),
        (Value::from("rate"), Value::from(1.0f64)),
    ]);
    assert!(client.request("update_player_timeline", &[Value::from(14u64), t_missing_duration]).await.is_err());

    let t_missing_rate = Value::Map(vec![
        (Value::from("position_ms"), Value::from(1000u64)),
        (Value::from("update_unix_ms"), Value::from(0i64)),
        (Value::from("duration_ms"), Value::from(2000u64)),
    ]);
    assert!(client.request("update_player_timeline", &[Value::from(15u64), t_missing_rate]).await.is_err());

    // wrong types in fields
    let t_bad_types = Value::Map(vec![
        (Value::from("position_ms"), Value::from("1000")),
        (Value::from("update_unix_ms"), Value::from("0")),
        (Value::from("duration_ms"), Value::from("2000")),
        (Value::from("rate"), Value::from("1.0")),
    ]);
    assert!(client.request("update_player_timeline", &[Value::from(16u64), t_bad_types]).await.is_err());

    // ---- update_player_metadata ----
    // wrong param counts
    assert!(client.request("update_player_metadata", &[]).await.is_err());
    assert!(client.request("update_player_metadata", &[Value::from(1u64)]).await.is_err());
    assert!(client.request("update_player_metadata", &[Value::from(1u64), Value::from(1u64)]).await.is_err());
    assert!(client.request("update_player_metadata", &[Value::from(1u64), Value::from(1u64), Value::Nil, Value::Nil]).await.is_err());

    // invalid player_id
    assert!(client.request("update_player_metadata", &[Value::from(0u64), Value::from(0x01u64), Value::Nil]).await.is_err());
    assert!(client.request("update_player_metadata", &[Value::from("1"), Value::from(0x01u64), Value::Nil]).await.is_err());

    // invalid metadata id code
    assert!(client.request("update_player_metadata", &[Value::from(1u64), Value::from(0u64), Value::Nil]).await.is_err());
    assert!(client.request("update_player_metadata", &[Value::from(1u64), Value::from(0xFFu64), Value::Nil]).await.is_err());

    // metadata id wrong type
    assert!(client.request("update_player_metadata", &[Value::from(1u64), Value::from("title"), Value::Nil]).await.is_err());

    // text wrong type
    assert!(client.request("update_player_metadata", &[Value::from(1u64), Value::from(0x01u64), Value::from(123u64)]).await.is_err());

    // ---- set_preferred_player ----
    // wrong param counts
    assert!(client.request("set_preferred_player", &[]).await.is_err());
    assert!(client.request("set_preferred_player", &[Value::from(1u64), Value::Nil]).await.is_err());
    // wrong type
    assert!(client.request("set_preferred_player", &[Value::from("1")]).await.is_err());
    // zero id invalid
    assert!(client.request("set_preferred_player", &[Value::from(0u64)]).await.is_err());

    // ---- get_preferred_player ----
    // wrong param counts
    assert!(client.request("get_preferred_player", &[Value::from(1u64)]).await.is_err());

    // ---- get_player_assigned_device ----
    // wrong param counts
    assert!(client.request("get_player_assigned_device", &[]).await.is_err());
    assert!(client.request("get_player_assigned_device", &[Value::from(1u64), Value::from(2u64)]).await.is_err());
    // invalid player_id types
    assert!(client.request("get_player_assigned_device", &[Value::from("1")]).await.is_err());
    assert!(client.request("get_player_assigned_device", &[Value::Nil]).await.is_err());
    assert!(client.request("get_player_assigned_device", &[Value::from(0u64)]).await.is_err());

    // shared: invalid player_id type (nil)
    assert!(client.request("update_player_status", &[Value::Nil, Value::from(1u64)]).await.is_err());

    server_task.shutdown().await?;
    Ok(())
}


#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_player_id_scope_validation_errors() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // Create a mock driver that only allows registration (so we can get a valid, registered id).
    // All other methods remain disabled and must NOT be called due to server-side validation.
    let mut um = helpers::FsctDriverMock::new();
    um.enable_register = true;
    um.fixed_id = std::num::NonZeroU32::new(50).unwrap();
    let mock = std::sync::Arc::new(um);

    let (client, server_task) = helpers::start_server_and_connect(mock.clone()).await;

    // Register a player to activate connection; we'll use a different, unregistered id for tests
    let _registered = client.register_player("p50".to_string()).await?;
    let bad_pid = std::num::NonZeroU32::new(999).unwrap();

    // 1) unregister_player should fail for unregistered id
    assert!(client.unregister_player(bad_pid).await.is_err());

    // 2) assign_player_to_device should fail
    let dev = uuid::Uuid::new_v4();
    assert!(client.assign_player_to_device(bad_pid, dev).await.is_err());

    // 3) unassign_player_from_device should fail
    assert!(client.unassign_player_from_device(bad_pid, dev).await.is_err());

    // 4) update_player_status should fail
    assert!(client.update_player_status(bad_pid, FsctStatus::Playing).await.is_err());

    // 5) update_player_timeline should fail
    let tl = TimelineInfo { position: std::time::Duration::from_millis(1), update_time: std::time::SystemTime::now(), duration: std::time::Duration::from_millis(2), rate: 1.0 };
    assert!(client.update_player_timeline(bad_pid, Some(tl)).await.is_err());

    // 6) update_player_metadata should fail
    assert!(client.update_player_metadata(bad_pid, FsctTextMetadata::CurrentTitle, Some("X".into())).await.is_err());

    // 7) update_player_state should fail
    let mut st = PlayerState::default();
    st.status = FsctStatus::Paused;
    assert!(client.update_player_state(bad_pid, st).await.is_err());

    // 8) get_player_assigned_device should fail
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

    // Scope the client so dropping ends the connection
    let server_task;
    {
        let (client_local, st) = helpers::start_server_and_connect(mock.clone()).await;
        server_task = st;

        // register one player over the connection
        let p1 = client_local.register_player("p201".to_string()).await?;
        assert_eq!(p1.get(), 201);

        // drop client_local here at end of scope -> triggers connection close
    }

    // Allow some time for the server to process the disconnect and auto-unregister
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Validate that unregister was called for the registered id
    let calls = mock.unregister_calls.lock().unwrap().clone();
    assert_eq!(calls, vec![201]);

    server_task.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_get_detected_devices() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // Create test devices
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

    // Get detected devices
    let devices = client.get_detected_devices().await?;

    // Verify we got 2 devices
    assert_eq!(devices.len(), 2);

    // Verify device 1
    assert_eq!(devices[0].id, device1.id);
    assert_eq!(devices[0].name, device1.name);
    assert_eq!(devices[0].manufacturer, device1.manufacturer);
    assert_eq!(devices[0].vendor_id, device1.vendor_id);
    assert_eq!(devices[0].product_id, device1.product_id);
    assert_eq!(devices[0].serial_number, device1.serial_number);

    // Verify device 2
    assert_eq!(devices[1].id, device2.id);
    assert_eq!(devices[1].name, device2.name);
    assert_eq!(devices[1].manufacturer, device2.manufacturer);
    assert_eq!(devices[1].vendor_id, device2.vendor_id);
    assert_eq!(devices[1].product_id, device2.product_id);
    assert_eq!(devices[1].serial_number, device2.serial_number);

    server_task.shutdown().await?;
    Ok(())
}
