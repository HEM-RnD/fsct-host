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

//! IPC client (phase 3 minimal) using parity-tokio-ipc transport.
//! For now, only implements `get_protocol_version` method to interoperate with the msgpack-rpc server.

use anyhow::Error;
use async_trait::async_trait;
use parity_tokio_ipc::Endpoint;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio::sync::broadcast;

use crate::definitions::ProtocolVersion;
use crate::{FsctDriver};
use crate::{PlayerEvent, PlayerState, ManagedPlayerId};
use crate::device_manager::ManagedDeviceId;
use crate::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};

use msgpack_rpc::{Client, Value};

fn default_endpoint() -> String {
    if let Ok(override_ep) = std::env::var("FSCT_IPC_ENDPOINT") {
        if !override_ep.trim().is_empty() {
            return override_ep;
        }
    }
    #[cfg(windows)]
    { r"\\.\pipe\fsct_host_v1".to_string() }
    #[cfg(unix)]
    {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        format!("{base}/fsct/fsct.sock")
    }
}

/// IPC-backed implementation of FsctDriver.
pub struct IpcDriver {
    endpoint: String,
    // Minimal event channel to satisfy subscribe_player_events; not used yet.
    events_tx: broadcast::Sender<PlayerEvent>,
}

impl IpcDriver {
    /// Create a client using default endpoint or FSCT_IPC_ENDPOINT.
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(16);
        Self { endpoint: default_endpoint(), events_tx: tx }
    }

    /// Create a client with an explicit endpoint (useful for tests/examples).
    pub fn with_endpoint(endpoint: String) -> Self {
        let (tx, _rx) = broadcast::channel(16);
        Self { endpoint, events_tx: tx }
    }

    /// Perform a single msgpack-rpc request for get_protocol_version.
    pub async fn get_protocol_version(&self) -> Result<ProtocolVersion, Error> {
        // Establish connection per-call for now (simple, OK for phase 3 minimal).
        let stream = Endpoint::connect(self.endpoint.clone()).await
            .map_err(|e| anyhow::anyhow!("IPC connect error: {e}"))?;

        // Adapt tokio stream to futures::io traits required by msgpack-rpc
        let compat_stream = stream.compat();
        let client = Client::new(compat_stream);

        // Send request via msgpack-rpc
        let response: Value = client
            .request("get_protocol_version", &[])
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;

        // Parse result map { major, minor }
        let map = response.as_map().ok_or_else(|| anyhow::anyhow!("invalid response: expected map"))?;
        let major = map
            .iter()
            .find_map(|(k, v)| match k { Value::String(s) if s.as_str() == Some("major") => v.as_u64(), _ => None })
            .ok_or_else(|| anyhow::anyhow!("missing major"))?;
        let minor = map
            .iter()
            .find_map(|(k, v)| match k { Value::String(s) if s.as_str() == Some("minor") => v.as_u64(), _ => None })
            .ok_or_else(|| anyhow::anyhow!("missing minor"))?;

        Ok(ProtocolVersion { major: major as u16, minor: minor as u16 })
    }
}

#[async_trait]
impl FsctDriver for IpcDriver {
    async fn register_player(&self, _self_id: String) -> Result<ManagedPlayerId, Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    async fn unregister_player(&self, _player_id: ManagedPlayerId) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    async fn assign_player_to_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    async fn unassign_player_from_device(&self, _player_id: ManagedPlayerId, _device_id: ManagedDeviceId) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    async fn update_player_state(&self, _player_id: ManagedPlayerId, _new_state: PlayerState) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    async fn update_player_status(&self, _player_id: ManagedPlayerId, _new_status: FsctStatus) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    async fn update_player_timeline(&self, _player_id: ManagedPlayerId, _new_timeline: Option<TimelineInfo>) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    async fn update_player_metadata(&self, _player_id: ManagedPlayerId, _metadata_id: FsctTextMetadata, _new_text: Option<String>) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    fn set_preferred_player(&self, _preferred: Option<ManagedPlayerId>) -> Result<(), Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    fn get_preferred_player(&self) -> Option<ManagedPlayerId> { None }

    fn get_player_assigned_device(&self, _player_id: ManagedPlayerId) -> Result<Option<ManagedDeviceId>, Error> {
        Err(anyhow::anyhow!("not implemented in IpcDriver (phase 3)"))
    }

    fn subscribe_player_events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.events_tx.subscribe()
    }
}
