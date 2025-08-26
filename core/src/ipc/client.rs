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

use crate::definitions::ProtocolVersion;
use crate::{FsctDriver};
use crate::{PlayerState, ManagedPlayerId};
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
    // Underlying msgpack-rpc client bound to a persistent IPC stream
    client: msgpack_rpc::Client,
    negotiated_version: ProtocolVersion,
}

impl IpcDriver {
    /// Connect to the default endpoint (or FSCT_IPC_ENDPOINT) and verify protocol compatibility.
    pub async fn create() -> Result<Self, Error> {
        Self::connect_to_endpoint(default_endpoint()).await
    }

    /// Connect to a specific endpoint and verify protocol compatibility.
    pub async fn connect_to_endpoint(endpoint: String) -> Result<Self, Error> {
        // Establish persistent connection
        let stream = Endpoint::connect(endpoint.clone()).await
            .map_err(|e| anyhow::anyhow!("IPC connect error: {e}"))?;
        let compat_stream = stream.compat();
        let client = Client::new(compat_stream);

        // Handshake: fetch remote protocol version
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
        let negotiated_version = ProtocolVersion { major: major as u16, minor: minor as u16 };

        // Verify compatibility: major must match our supported major
        if negotiated_version.major != crate::FSCT_PROTOCOL_VERSION.major {
            return Err(anyhow::anyhow!(
                "incompatible protocol version: remote {}.{} != local {}.{}",
                negotiated_version.major,
                negotiated_version.minor,
                crate::FSCT_PROTOCOL_VERSION.major,
                crate::FSCT_PROTOCOL_VERSION.minor
            ));
        }

        Ok(Self { client, negotiated_version})
    }

    /// Returns the negotiated protocol version obtained during creation.
    pub async fn get_protocol_version(&self) -> Result<ProtocolVersion, Error> {
        Ok(self.negotiated_version)
    }
}

#[async_trait]
impl FsctDriver for IpcDriver {
    async fn register_player(&self, self_id: String) -> Result<ManagedPlayerId, Error> {
        let response: Value = self
            .client
            .request("register_player", &[Value::from(self_id)])
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        let id64 = response
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("invalid response for register_player: expected integer"))?;
        let id_u32 = id64 as u32;
        let nz = std::num::NonZeroU32::new(id_u32)
            .ok_or_else(|| anyhow::anyhow!("server returned invalid zero player id"))?;
        Ok(nz)
    }

    async fn unregister_player(&self, player_id: ManagedPlayerId) -> Result<(), Error> {
        let _response: Value = self
            .client
            .request("unregister_player", &[Value::from(player_id.get() as u64)])
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        let _response: Value = self
            .client
            .request(
                "assign_player_to_device",
                &[Value::from(player_id.get() as u64), Value::Binary(device_id.as_bytes().to_vec())],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        let _response: Value = self
            .client
            .request(
                "unassign_player_from_device",
                &[Value::from(player_id.get() as u64), Value::Binary(device_id.as_bytes().to_vec())],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
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
}
