// Integration test for IPC server<->client assign/unassign player to device using mock driver
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fsct_core::ipc::server::IpcServer;
use fsct_core::ipc::client::IpcDriver;
use fsct_core::FsctDriver;
use fsct_core::{ManagedDeviceId, ManagedPlayerId};
use fsct_core::player_state::PlayerState;
use fsct_core::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use async_trait::async_trait;

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

// Mock driver capturing assign/unassign calls
struct MockDriver {
    assign_calls: Mutex<Vec<(u32, uuid::Uuid)>>,
    unassign_calls: Mutex<Vec<(u32, uuid::Uuid)>>,
}

impl MockDriver {
    fn new() -> Self {
        Self { assign_calls: Mutex::new(Vec::new()), unassign_calls: Mutex::new(Vec::new()) }
    }
}

#[async_trait]
impl FsctDriver for MockDriver {
    async fn register_player(&self, _self_id: String) -> anyhow::Result<ManagedPlayerId, anyhow::Error> {
        Err(anyhow::anyhow!("not used in assign test"))
    }

    async fn unregister_player(&self, _player_id: ManagedPlayerId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in assign test"))
    }

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        self.assign_calls.lock().unwrap().push((player_id.get(), device_id));
        Ok(())
    }

    async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        self.unassign_calls.lock().unwrap().push((player_id.get(), device_id));
        Ok(())
    }

    async fn update_player_state(&self, _player_id: ManagedPlayerId, _new_state: PlayerState) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in assign test"))
    }

    async fn update_player_status(&self, _player_id: ManagedPlayerId, _new_status: FsctStatus) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in assign test"))
    }

    async fn update_player_timeline(&self, _player_id: ManagedPlayerId, _new_timeline: Option<TimelineInfo>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in assign test"))
    }

    async fn update_player_metadata(&self, _player_id: ManagedPlayerId, _metadata_id: FsctTextMetadata, _new_text: Option<String>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in assign test"))
    }

    async fn set_preferred_player(&self, _preferred: Option<ManagedPlayerId>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in assign test"))
    }

    async fn get_preferred_player(&self) -> Option<ManagedPlayerId> { None }

    async fn get_player_assigned_device(&self, _player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>, anyhow::Error> {
        Ok(None)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_assign_and_unassign_player() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let endpoint = test_endpoint();

    let mock = Arc::new(MockDriver::new());
    let driver_trait_obj: Arc<dyn FsctDriver> = mock.clone();

    // Start server
    let server = IpcServer::with_endpoint(driver_trait_obj, endpoint.clone());
    let server_task = tokio::spawn(async move {
        let _ = server.serve().await;
    });

    // Connect client with retry
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);
    let client = loop {
        match IpcDriver::connect_to_endpoint(endpoint.clone()).await {
            Ok(c) => break c,
            Err(_) => {
                if start.elapsed() > timeout {
                    server_task.abort();
                    panic!("Failed to connect to IPC server in time");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };

    // Prepare ids
    let player_id = std::num::NonZeroU32::new(42).unwrap();
    let device_id = uuid::Uuid::new_v4();

    // Assign
    client.assign_player_to_device(player_id, device_id).await?;
    {
        let calls = mock.assign_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "assign should be called exactly once");
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, device_id);
    }

    // Unassign
    client.unassign_player_from_device(player_id, device_id).await?;
    {
        let calls = mock.unassign_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "unassign should be called exactly once");
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, device_id);
    }

    server_task.abort();

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(endpoint);
    }

    Ok(())
}
