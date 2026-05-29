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

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fsct::FSCT_PROTOCOL_VERSION;
use fsct::FsctDriver;
use fsct_client::IpcDriver;
use fsct_driver::IpcServer;

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
    async fn register_player(&self, _self_id: String) -> Result<fsct::ManagedPlayerId, anyhow::Error> {
        Err(anyhow::anyhow!("not used"))
    }
    async fn unregister_player(&self, _player_id: fsct::ManagedPlayerId) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn assign_player_to_device(
        &self,
        _player_id: fsct::ManagedPlayerId,
        _device_id: fsct::ManagedDeviceId,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn unassign_player_from_device(
        &self,
        _player_id: fsct::ManagedPlayerId,
        _device_id: fsct::ManagedDeviceId,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn update_player_state(
        &self,
        _player_id: fsct::ManagedPlayerId,
        _new_state: fsct::player_state::PlayerState,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn update_player_status(
        &self,
        _player_id: fsct::ManagedPlayerId,
        _new_status: fsct::definitions::FsctStatus,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn update_player_timeline(
        &self,
        _player_id: fsct::ManagedPlayerId,
        _new_timeline: Option<fsct::definitions::TimelineInfo>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn update_player_metadata(
        &self,
        _player_id: fsct::ManagedPlayerId,
        _metadata_id: fsct::definitions::FsctTextMetadata,
        _new_text: Option<String>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn get_player_assigned_device(
        &self,
        _player_id: fsct::ManagedPlayerId,
    ) -> Result<Option<fsct::ManagedDeviceId>, anyhow::Error> {
        Ok(None)
    }
    async fn get_detected_devices(&self) -> Result<Vec<fsct::ManagedDeviceId>, anyhow::Error> {
        Ok(Vec::new())
    }
    async fn subscribe_device_changes(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<fsct::DeviceChangeEvent>, anyhow::Error> {
        let (_tx, rx) = tokio::sync::broadcast::channel(1);
        Ok(rx)
    }
    async fn get_device_info(
        &self,
        _device_id: fsct::ManagedDeviceId,
    ) -> Result<fsct::definitions::DeviceInfo, anyhow::Error> {
        Err(anyhow::anyhow!("not used"))
    }
    async fn get_timesync(&self) -> Result<fsct::definitions::TimeSync, anyhow::Error> {
        Ok(fsct::definitions::TimeSync::sample_now())
    }
}

#[tokio::test]
async fn two_clients_can_connect_and_request_version() {
    let _ = env_logger::try_init();
    let endpoint = test_endpoint();
    let driver: Arc<dyn FsctDriver> = Arc::new(TestDriver);
    let mut server = IpcServer::with_socket_path(driver, endpoint.as_str());
    // run server in background
    let server_task = fsct_driver::spawn_service(async move |mut s| {
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
