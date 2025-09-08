use std::sync::Arc;
use std::time::Duration;

use fsct_core::ipc::client::IpcDriver;
use fsct_core::ipc::server::IpcServer;
use fsct_core::FsctDriver;
use async_trait::async_trait;
use fsct_core::FSCT_PROTOCOL_VERSION;

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

struct TestDriver;

#[async_trait]
impl FsctDriver for TestDriver {
    async fn register_player(&self, _self_id: String) -> Result<fsct_core::ManagedPlayerId, anyhow::Error> {
        Err(anyhow::anyhow!("not used"))
    }
    async fn unregister_player(&self, _player_id: fsct_core::ManagedPlayerId) -> Result<(), anyhow::Error> { Ok(()) }
    async fn assign_player_to_device(&self, _player_id: fsct_core::ManagedPlayerId, _device_id: fsct_core::ManagedDeviceId) -> Result<(), anyhow::Error> { Ok(()) }
    async fn unassign_player_from_device(&self, _player_id: fsct_core::ManagedPlayerId, _device_id: fsct_core::ManagedDeviceId) -> Result<(), anyhow::Error> { Ok(()) }
    async fn update_player_state(&self, _player_id: fsct_core::ManagedPlayerId, _new_state: fsct_core::player_state::PlayerState) -> Result<(), anyhow::Error> { Ok(()) }
    async fn update_player_status(&self, _player_id: fsct_core::ManagedPlayerId, _new_status: fsct_core::definitions::FsctStatus) -> Result<(), anyhow::Error> { Ok(()) }
    async fn update_player_timeline(&self, _player_id: fsct_core::ManagedPlayerId, _new_timeline: Option<fsct_core::definitions::TimelineInfo>) -> Result<(), anyhow::Error> { Ok(()) }
    async fn update_player_metadata(&self, _player_id: fsct_core::ManagedPlayerId, _metadata_id: fsct_core::definitions::FsctTextMetadata, _new_text: Option<String>) -> Result<(), anyhow::Error> { Ok(()) }
    async fn set_preferred_player(&self, _preferred: Option<fsct_core::ManagedPlayerId>) -> Result<(), anyhow::Error> { Ok(()) }
    async fn get_preferred_player(&self) -> Option<fsct_core::ManagedPlayerId> { None }
    async fn get_player_assigned_device(&self, _player_id: fsct_core::ManagedPlayerId) -> Result<Option<fsct_core::ManagedDeviceId>, anyhow::Error> { Ok(None) }
}

#[tokio::test]
async fn two_clients_can_connect_and_request_version() {
    let _ = env_logger::try_init();
    let endpoint = test_endpoint();
    let driver: Arc<dyn FsctDriver> = Arc::new(TestDriver);
    let mut server = IpcServer::with_socket_path(driver, endpoint.as_str());
    // run server in background
    let server_task = fsct_core::spawn_service(async move |mut s| {
        tokio::select! {
            _ = server.serve() => (),
            _ = s.signaled() => (),
        }
        server.shutdown().await;
    });

    // Retry connect until server is up
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);
    let c1 = loop {
        match IpcDriver::connect_to_endpoint(endpoint.clone()).await {
            Ok(c) => break c,
            Err(_) if start.elapsed() < timeout => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(e) => panic!("client1 connect failed: {}", e),
        }
    };
    let c2 = loop {
        match IpcDriver::connect_to_endpoint(endpoint.clone()).await {
            Ok(c) => break c,
            Err(_) if start.elapsed() < timeout => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(e) => panic!("client2 connect failed: {}", e),
        }
    };

    // Just call get_protocol_version through the driver which during connect already fetched it; we can rely on negotiated version
    assert_eq!(c1.get_protocol_version().await.unwrap(), FSCT_PROTOCOL_VERSION);
    assert_eq!(c2.get_protocol_version().await.unwrap(), FSCT_PROTOCOL_VERSION);

    server_task.shutdown().await.unwrap();
}
