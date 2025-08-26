// Integration test for IPC update methods: state, status, timeline, metadata
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use fsct_core::ipc::server::IpcServer;
use fsct_core::ipc::client::IpcDriver;
use fsct_core::FsctDriver;
use fsct_core::{ManagedDeviceId, ManagedPlayerId};
use fsct_core::player_state::{PlayerState, TrackMetadata};
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

#[derive(Default)]
struct CapturedState {
    pub state_calls: Mutex<Vec<(u32, PlayerState)>>,
    pub status_calls: Mutex<Vec<(u32, FsctStatus)>>,
    pub timeline_calls: Mutex<Vec<(u32, Option<TimelineInfo>)>>,
    pub metadata_calls: Mutex<Vec<(u32, FsctTextMetadata, Option<String>)>>,
}

struct MockDriver {
    cap: Arc<CapturedState>,
}

impl MockDriver {
    fn new() -> (Self, Arc<CapturedState>) {
        let cap = Arc::new(CapturedState::default());
        (Self { cap: cap.clone() }, cap)
    }
}

#[async_trait]
impl FsctDriver for MockDriver {
    async fn register_player(&self, _self_id: String) -> anyhow::Result<ManagedPlayerId, anyhow::Error> {
        Err(anyhow::anyhow!("not used in update test"))
    }

    async fn unregister_player(&self, _player_id: ManagedPlayerId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in update test"))
    }

    async fn assign_player_to_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in update test"))
    }

    async fn unassign_player_from_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in update test"))
    }

    async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> anyhow::Result<(), anyhow::Error> {
        self.cap.state_calls.lock().unwrap().push((player_id.get(), new_state));
        Ok(())
    }

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> anyhow::Result<(), anyhow::Error> {
        self.cap.status_calls.lock().unwrap().push((player_id.get(), new_status));
        Ok(())
    }

    async fn update_player_timeline(&self, player_id: ManagedPlayerId, new_timeline: Option<TimelineInfo>) -> anyhow::Result<(), anyhow::Error> {
        self.cap.timeline_calls.lock().unwrap().push((player_id.get(), new_timeline));
        Ok(())
    }

    async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: Option<String>) -> anyhow::Result<(), anyhow::Error> {
        self.cap.metadata_calls.lock().unwrap().push((player_id.get(), metadata_id, new_text));
        Ok(())
    }

    fn set_preferred_player(&self, _preferred: Option<ManagedPlayerId>) -> anyhow::Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("not used in update test"))
    }

    fn get_preferred_player(&self) -> Option<ManagedPlayerId> { None }

    fn get_player_assigned_device(&self, _player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>, anyhow::Error> {
        Ok(None)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_update_methods() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let endpoint = test_endpoint();

    let (mock, cap) = MockDriver::new();
    let driver_trait_obj: Arc<dyn FsctDriver> = Arc::new(mock);

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

    let player_id = std::num::NonZeroU32::new(7).unwrap();

    // update_player_status
    client.update_player_status(player_id, FsctStatus::Playing).await?;
    {
        let calls = cap.status_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, FsctStatus::Playing);
    }

    // update_player_timeline
    let timeline = TimelineInfo {
        position: Duration::from_millis(12345),
        update_time: SystemTime::now(),
        duration: Duration::from_millis(54321),
        rate: 1.0,
    };
    client.update_player_timeline(player_id, Some(timeline.clone())).await?;
    {
        let calls = cap.timeline_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert!(calls[0].1.is_some());
        let got = calls[0].1.as_ref().unwrap();
        assert_eq!(got.position, timeline.position);
        assert_eq!(got.duration, timeline.duration);
        assert!((got.rate - timeline.rate).abs() < 1e-9);
    }

    // update_player_metadata
    client.update_player_metadata(player_id, FsctTextMetadata::CurrentTitle, Some("Track X".to_string())).await?;
    {
        let calls = cap.metadata_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1, FsctTextMetadata::CurrentTitle);
        assert_eq!(calls[0].2.as_deref(), Some("Track X"));
    }

    // update_player_state (full)
    let mut state = PlayerState::default();
    state.status = FsctStatus::Paused;
    state.timeline = None; // ensure None goes through
    state.texts = TrackMetadata { title: Some("T".into()), artist: None, album: Some("Alb".into()), genre: None };
    client.update_player_state(player_id, state.clone()).await?;
    {
        let calls = cap.state_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, player_id.get());
        assert_eq!(calls[0].1.status, state.status);
        assert_eq!(calls[0].1.timeline, state.timeline);
        assert_eq!(calls[0].1.texts.title, state.texts.title);
        assert_eq!(calls[0].1.texts.album, state.texts.album);
    }

    server_task.abort();

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(endpoint);
    }

    Ok(())
}
