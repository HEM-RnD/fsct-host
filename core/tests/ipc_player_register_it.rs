// Integration test for IPC server<->client register/unregister player with mock driver verifying calls
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fsct_core::ipc::server::IpcServer;
use fsct_core::ipc::client::IpcDriver;
use fsct_core::FsctDriver;
use fsct_core::ManagedDeviceId;
use fsct_core::ManagedPlayerId;
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

// Simple thread-safe mock implementing FsctDriver, capturing calls and returning a fixed player id.
struct MockDriver {
    register_calls: Mutex<Vec<String>>,        // captured self_id strings
    unregister_calls: Mutex<Vec<u32>>,         // captured player ids
    fixed_id: ManagedPlayerId,                 // id to return from register
}

impl MockDriver {
    fn new(fixed_id: ManagedPlayerId) -> Self {
        Self {
            register_calls: Mutex::new(Vec::new()),
            unregister_calls: Mutex::new(Vec::new()),
            fixed_id,
        }
    }
}

#[async_trait]
impl FsctDriver for MockDriver {
    async fn register_player(&self, self_id: String) -> anyhow::Result<ManagedPlayerId, anyhow::Error> {
        self.register_calls.lock().unwrap().push(self_id);
        Ok(self.fixed_id)
    }

    async fn unregister_player(&self, player_id: ManagedPlayerId) -> anyhow::Result<(), anyhow::Error> {
        self.unregister_calls.lock().unwrap().push(player_id.get());
        Ok(())
    }

    async fn assign_player_to_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }

    async fn unassign_player_from_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }

    async fn update_player_state(&self, _player_id: ManagedPlayerId, _new_state: PlayerState) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }

    async fn update_player_status(&self, _player_id: ManagedPlayerId, _new_status: FsctStatus) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }

    async fn update_player_timeline(&self, _player_id: ManagedPlayerId, _new_timeline: Option<TimelineInfo>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }

    async fn update_player_metadata(&self, _player_id: ManagedPlayerId, _metadata_id: FsctTextMetadata, _new_text: Option<String>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }

    fn set_preferred_player(&self, _preferred: Option<ManagedPlayerId>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }

    fn get_preferred_player(&self) -> Option<ManagedPlayerId> { None }

    fn get_player_assigned_device(&self, _player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>, anyhow::Error> {
        Err(anyhow::anyhow!("not used in mock"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_register_and_unregister_player() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let endpoint = test_endpoint();

    // Prepare mock driver that will return a fixed id
    let fixed_id = std::num::NonZeroU32::new(123).unwrap();
    let mock = Arc::new(MockDriver::new(fixed_id));
    let driver_trait_obj: Arc<dyn FsctDriver> = mock.clone();

    // Start server with MockDriver
    let server = IpcServer::with_endpoint(driver_trait_obj, endpoint.clone());

    let server_task = tokio::spawn(async move {
        let _ = server.serve().await;
    });

    // Retry connect loop until server ready
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

    // Register a player and verify returned id
    let self_id = "test_player_self_id".to_string();
    let player_id = client.register_player(self_id.clone()).await?;
    assert_eq!(player_id, fixed_id, "Returned player id should match mock's fixed id");

    // Verify the mock recorded the register call with correct parameter
    {
        let calls = mock.register_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "register_player should be called exactly once");
        assert_eq!(calls[0], self_id, "self_id should be forwarded correctly");
    }

    // Unregister the player and verify the mock recorded the call
    client.unregister_player(player_id).await?;
    {
        let calls = mock.unregister_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "unregister_player should be called exactly once");
        assert_eq!(calls[0], fixed_id.get(), "unregister should be called with the same id returned by register");
    }

    // Stop server
    server_task.abort();

    // Clean up leftover unix domain socket file if any
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(endpoint);
    }

    Ok(())
}
