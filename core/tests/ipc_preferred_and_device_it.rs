// Integration test for IPC methods: set/get preferred player and get_player_assigned_device
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fsct_core::FsctDriver;
use fsct_core::ipc::client::IpcDriver;
use fsct_core::ipc::server::IpcServer;
use fsct_core::{ManagedDeviceId, ManagedPlayerId};
use fsct_core::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use fsct_core::player_state::PlayerState;

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

struct MockDriver {
    // capture set_preferred calls
    set_calls: Mutex<Vec<Option<u32>>>,
    // current preferred to return from get_preferred_player
    current_preferred: Mutex<Option<ManagedPlayerId>>,
    // mapping for get_player_assigned_device
    assigned_device: Mutex<Option<uuid::Uuid>>,
    last_assigned_query: Mutex<Vec<u32>>,
}

impl MockDriver {
    fn new() -> Self {
        Self {
            set_calls: Mutex::new(Vec::new()),
            current_preferred: Mutex::new(None),
            assigned_device: Mutex::new(None),
            last_assigned_query: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl FsctDriver for MockDriver {
    async fn register_player(&self, _self_id: String) -> anyhow::Result<ManagedPlayerId, anyhow::Error> { Err(anyhow::anyhow!("unused")) }
    async fn unregister_player(&self, _player_id: ManagedPlayerId) -> anyhow::Result<(), anyhow::Error> { Err(anyhow::anyhow!("unused")) }
    async fn assign_player_to_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> { Err(anyhow::anyhow!("unused")) }
    async fn unassign_player_from_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> { Err(anyhow::anyhow!("unused")) }
    async fn update_player_state(&self, _player_id: ManagedPlayerId, _new_state: PlayerState) -> anyhow::Result<(), anyhow::Error> { Err(anyhow::anyhow!("unused")) }
    async fn update_player_status(&self, _player_id: ManagedPlayerId, _new_status: FsctStatus) -> anyhow::Result<(), anyhow::Error> { Err(anyhow::anyhow!("unused")) }
    async fn update_player_timeline(&self, _player_id: ManagedPlayerId, _new_timeline: Option<TimelineInfo>) -> anyhow::Result<(), anyhow::Error> { Err(anyhow::anyhow!("unused")) }
    async fn update_player_metadata(&self, _player_id: ManagedPlayerId, _metadata_id: FsctTextMetadata, _new_text: Option<String>) -> anyhow::Result<(), anyhow::Error> { Err(anyhow::anyhow!("unused")) }

    async fn set_preferred_player(&self, preferred: Option<ManagedPlayerId>) -> anyhow::Result<(), anyhow::Error> {
        self.set_calls.lock().unwrap().push(preferred.map(|p| p.get()));
        *self.current_preferred.lock().unwrap() = preferred;
        Ok(())
    }

    async fn get_preferred_player(&self) -> Option<ManagedPlayerId> {
        *self.current_preferred.lock().unwrap()
    }

    async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>,
    anyhow::Error> {
        self.last_assigned_query.lock().unwrap().push(player_id.get());
        Ok(*self.assigned_device.lock().unwrap())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_preferred_and_assigned_device_methods() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let endpoint = test_endpoint();

    let mock = Arc::new(MockDriver::new());
    let server = IpcServer::with_endpoint(mock.clone() as Arc<dyn FsctDriver>, endpoint.clone());
    let server_task = tokio::spawn(async move { let _ = server.serve().await; });

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

    let p1 = std::num::NonZeroU32::new(101).unwrap();

    // set_preferred_player(Some)
    client.set_preferred_player(Some(p1)).await?;
    {
        let calls = mock.set_calls.lock().unwrap().clone();
        assert_eq!(calls, vec![Some(p1.get())]);
    }

    // get_preferred_player -> should return p1
    let got = client.get_preferred_player().await;
    assert_eq!(got, Some(p1));

    // set_preferred_player(None) and verify
    client.set_preferred_player(None).await?;
    {
        let calls = mock.set_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1], None);
    }
    let got2 = client.get_preferred_player().await;
    assert_eq!(got2, None);

    // get_player_assigned_device
    let device = uuid::Uuid::new_v4();
    *mock.assigned_device.lock().unwrap() = Some(device);
    let q = std::num::NonZeroU32::new(55).unwrap();
    let dev = client.get_player_assigned_device(q).await?;
    assert_eq!(dev, Some(device));
    let queries = mock.last_assigned_query.lock().unwrap().clone();
    assert_eq!(queries.last().copied(), Some(q.get()));

    server_task.abort();

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(endpoint);
    }

    Ok(())
}
