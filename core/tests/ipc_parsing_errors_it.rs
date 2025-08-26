// Integration tests to verify that server-side parsing errors are returned to the client
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fsct_core::ipc::server::IpcServer;
use fsct_core::{FsctDriver, ManagedDeviceId, ManagedPlayerId};
use fsct_core::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use fsct_core::player_state::PlayerState;
use msgpack_rpc::{Client, Value};
use parity_tokio_ipc::Endpoint;
use tokio_util::compat::TokioAsyncReadCompatExt;

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

struct NoopDriver;

#[async_trait]
impl FsctDriver for NoopDriver {
    async fn register_player(&self, _self_id: String) -> anyhow::Result<ManagedPlayerId, anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn unregister_player(&self, _player_id: ManagedPlayerId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn assign_player_to_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn unassign_player_from_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn update_player_state(&self, _player_id: ManagedPlayerId, _new_state: PlayerState) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn update_player_status(&self, _player_id: ManagedPlayerId, _new_status: FsctStatus) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn update_player_timeline(&self, _player_id: ManagedPlayerId, _new_timeline: Option<TimelineInfo>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn update_player_metadata(&self, _player_id: ManagedPlayerId, _metadata_id: FsctTextMetadata, _new_text: Option<String>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not expected to be called"))
    }
    async fn set_preferred_player(&self, _preferred: Option<ManagedPlayerId>) -> anyhow::Result<(), anyhow::Error> { Err
    (anyhow::anyhow!("not expected")) }
    async fn get_preferred_player(&self) -> Option<ManagedPlayerId> { None }
    async fn get_player_assigned_device(&self, _player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>,
    anyhow::Error> { Ok(None) }
}

async fn connect_raw_client(endpoint: String) -> Client {
    let stream = Endpoint::connect(endpoint).await.expect("connect ok");
    let compat = stream.compat();
    Client::new(compat)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_parsing_errors_are_returned_to_client() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let endpoint = test_endpoint();

    // Start server with NoopDriver (should not be called on parse errors)
    let driver: Arc<dyn FsctDriver> = Arc::new(NoopDriver);
    let server = IpcServer::with_endpoint(driver, endpoint.clone());
    let server_task = tokio::spawn(async move { let _ = server.serve().await; });

    // Retry connect loop until server ready
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);
    let client = loop {
        match Endpoint::connect(endpoint.clone()).await {
            Ok(stream) => break Client::new(stream.compat()),
            Err(_) => {
                if start.elapsed() > timeout {
                    server_task.abort();
                    panic!("Failed to connect to IPC server in time");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };

    // ---- update_player_state ----
    // wrong param counts
    assert!(client.request("update_player_state", &[]).await.is_err());
    assert!(client.request("update_player_state", &[Value::from(1u64)]).await.is_err());
    assert!(client.request("update_player_state", &[Value::from(1u64), Value::Map(vec![]), Value::Nil]).await.is_err());

    // invalid player_id types
    assert!(client.request("update_player_state", &[Value::from("1"), Value::Map(vec![])]).await.is_err());
    assert!(client.request("update_player_state", &[Value::from(0u64), Value::Map(vec![])]).await.is_err());

    // 1) wrong key name in state map
    let bad_state = Value::Map(vec![
        (Value::from("statsu"), Value::from(1u64)), // typo; reject
        (Value::from("texts"), Value::Map(vec![])),
        (Value::from("timeline"), Value::Nil),
    ]);
    assert!(client.request("update_player_state", &[Value::from(1u64), bad_state]).await.is_err());

    // 2) unknown texts sub-key
    let bad_state2 = Value::Map(vec![
        (Value::from("status"), Value::from(1u64)),
        (Value::from("texts"), Value::Map(vec![ (Value::from("titl"), Value::from("X")) ])),
        (Value::from("timeline"), Value::Nil),
    ]);
    assert!(client.request("update_player_state", &[Value::from(2u64), bad_state2]).await.is_err());

    // 3) texts has wrong value types
    let bad_state3 = Value::Map(vec![
        (Value::from("status"), Value::from(1u64)),
        (Value::from("texts"), Value::Map(vec![ (Value::from("title"), Value::from(123u64)) ])),
        (Value::from("timeline"), Value::Nil),
    ]);
    assert!(client.request("update_player_state", &[Value::from(3u64), bad_state3]).await.is_err());

    // 4) timeline present but not a map
    let bad_state4 = Value::Map(vec![ (Value::from("timeline"), Value::from(1u64)) ]);
    assert!(client.request("update_player_state", &[Value::from(4u64), bad_state4]).await.is_err());

    // 5) status wrong type
    let bad_state5 = Value::Map(vec![ (Value::from("status"), Value::from("x")) ]);
    assert!(client.request("update_player_state", &[Value::from(5u64), bad_state5]).await.is_err());

    // 6) status invalid code
    let bad_state6 = Value::Map(vec![ (Value::from("status"), Value::from(255u64)) ]);
    assert!(client.request("update_player_state", &[Value::from(6u64), bad_state6]).await.is_err());

    // 7) timeline inside state missing fields
    let bad_state7 = Value::Map(vec![ (Value::from("timeline"), Value::Map(vec![ (Value::from("position_ms"), Value::from(1u64)) ])) ]);
    assert!(client.request("update_player_state", &[Value::from(7u64), bad_state7]).await.is_err());

    // 8) texts not a map
    let bad_state8 = Value::Map(vec![ (Value::from("texts"), Value::from(5u64)) ]);
    assert!(client.request("update_player_state", &[Value::from(8u64), bad_state8]).await.is_err());

    // 9) non-map state
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
    let bad_timeline2 = Value::Map(vec![ (Value::from("pos"), Value::from(1u64)) ]);
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
    // nil is allowed (no error) - but we don't assert here because driver returns ok; we just ensure error cases are covered

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

    // ---- shared: invalid player_id type (nil)
    assert!(client.request("update_player_status", &[Value::Nil, Value::from(1u64)]).await.is_err());

    server_task.abort();

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(endpoint);
    }

    Ok(())
}