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

//! IPC client using platform-native Tokio transports (Unix sockets / Windows named pipes).

use anyhow::Error;
use async_trait::async_trait;
use tokio_util::compat::TokioAsyncReadCompatExt;

use fsct::definitions::ProtocolVersion;
use fsct::{FsctDriver};
use fsct::{PlayerState, ManagedPlayerId};
use fsct::device_manager::ManagedDeviceId;
use fsct::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};

use msgpack_rpc::{Client, Value};

fn encode_status(s: FsctStatus) -> Value { Value::from(s as u64) }

fn encode_timeline_opt(t: &Option<TimelineInfo>) -> Value {
    match t {
        None => Value::Nil,
        Some(tl) => {
            let mut map = Vec::new();
            map.push((Value::from("position_ms"), Value::from(tl.position.as_millis() as u64)));
            let update_ms = tl.update_time.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_else(|e| -(e.duration().as_millis() as i64));
            map.push((Value::from("update_unix_ms"), Value::from(update_ms)));
            map.push((Value::from("duration_ms"), Value::from(tl.duration.as_millis() as u64)));
            map.push((Value::from("rate"), Value::from(tl.rate)));
            Value::Map(map)
        }
    }
}

fn encode_text_metadata_id(m: FsctTextMetadata) -> Value { Value::from(m as u64) }

fn encode_optional<V: Into<Value>>(v: Option<V>) -> Value {
    match v {
        None => Value::Nil,
        Some(v) => v.into()
    }
}

fn encode_optional_string(v: &Option<String>) -> Value {
    encode_optional(v.as_ref().map(|s| s.as_str()))
}

fn encode_optional_field(name: &str, v: &Option<String>) -> (Value, Value) {
    (Value::from(name), encode_optional_string(v))
}

fn encode_player_state(ps: &PlayerState) -> Value {
    let mut map = Vec::new();
    map.push((Value::from("status"), encode_status(ps.status)));
    map.push((Value::from("timeline"), encode_timeline_opt(&ps.timeline)));
    let mut texts = Vec::new();
    texts.push(encode_optional_field("title", &ps.texts.title));
    texts.push(encode_optional_field("artist", &ps.texts.artist));
    texts.push(encode_optional_field("album", &ps.texts.album));
    texts.push(encode_optional_field("genre", &ps.texts.genre));
    map.push((Value::from("texts"), Value::Map(texts)));
    Value::Map(map)
}

fn encode_player_id(pid: ManagedPlayerId) -> Value { Value::from(pid.get() as u64) }

/// IPC-backed implementation of FsctDriver.
pub struct IpcDriver {
    // Underlying msgpack-rpc client bound to a persistent IPC stream
    client: msgpack_rpc::Client,
    negotiated_version: ProtocolVersion,
}

impl IpcDriver {
    /// Connect to a specific endpoint and verify protocol compatibility.
    pub async fn connect_to_endpoint(endpoint: String) -> Result<Self, Error> {
        // Establish persistent connection
        let stream = transport::EndpointClient::connect(endpoint.clone()).await
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
            .find_map(|(k, v)| match k {
                Value::String(s) if s.as_str() == Some("major") => v.as_u64(),
                _ => None
            })
            .ok_or_else(|| anyhow::anyhow!("missing major"))?;
        let minor = map
            .iter()
            .find_map(|(k, v)| match k {
                Value::String(s) if s.as_str() == Some("minor") => v.as_u64(),
                _ => None
            })
            .ok_or_else(|| anyhow::anyhow!("missing minor"))?;
        let negotiated_version = ProtocolVersion { major: major as u16, minor: minor as u16 };

        // Verify compatibility: major must match our supported major
        if negotiated_version.major != fsct::FSCT_PROTOCOL_VERSION.major {
            return Err(anyhow::anyhow!(
                "incompatible protocol version: remote {}.{} != local {}.{}",
                negotiated_version.major,
                negotiated_version.minor,
                fsct::FSCT_PROTOCOL_VERSION.major,
                fsct::FSCT_PROTOCOL_VERSION.minor
            ));
        }

        Ok(Self { client, negotiated_version })
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
            .request("unregister_player", &[encode_player_id(player_id)])
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        let _response: Value = self
            .client
            .request(
                "assign_player_to_device",
                &[encode_player_id(player_id), Value::Binary(device_id.as_bytes().to_vec())],
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
                &[encode_player_id(player_id), Value::Binary(device_id.as_bytes().to_vec())],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> Result<(), Error> {
        let state_val = encode_player_state(&new_state);
        let _response: Value = self
            .client
            .request(
                "update_player_state",
                &[encode_player_id(player_id), state_val],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> Result<(), Error> {
        let _response: Value = self
            .client
            .request(
                "update_player_status",
                &[encode_player_id(player_id), encode_status(new_status)],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn update_player_timeline(&self, player_id: ManagedPlayerId, new_timeline: Option<TimelineInfo>) -> Result<(), Error> {
        let _response: Value = self
            .client
            .request(
                "update_player_timeline",
                &[encode_player_id(player_id), encode_timeline_opt(&new_timeline)],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: Option<String>) -> Result<(), Error> {
        let _response: Value = self
            .client
            .request(
                "update_player_metadata",
                &[
                    encode_player_id(player_id),
                    encode_text_metadata_id(metadata_id),
                    encode_optional_string(&new_text),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        Ok(())
    }

    async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> Result<Option<ManagedDeviceId>, Error> {
        let resp: Value = self
            .client
            .request("get_player_assigned_device", &[encode_player_id(player_id)])
            .await
            .map_err(|e| anyhow::anyhow!("rpc request error: {e}"))?;
        if resp.is_nil() { return Ok(None); }
        let bytes = resp.as_slice().ok_or_else(|| anyhow::anyhow!("invalid response for get_player_assigned_device: expected binary or nil"))?;
        if bytes.len() != 16 { return Err(anyhow::anyhow!("invalid uuid length")); }
        let uuid = uuid::Uuid::from_slice(bytes).map_err(|e| anyhow::anyhow!("invalid uuid: {e}"))?;
        Ok(Some(uuid))
    }
}

mod transport {
    pub struct EndpointClient;

    impl EndpointClient {
        #[cfg(unix)]
        pub async fn connect(path: String) -> anyhow::Result<tokio::net::UnixStream> {
            use anyhow::Context;

            tokio::net::UnixStream::connect(path).await.context("unix client connect failed")
        }

        #[cfg(windows)]
        pub async fn connect(name: String) -> anyhow::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
            use std::time::Duration;
            use tokio::net::windows::named_pipe;
            use tokio::time::Instant;
            use winapi::shared::winerror::ERROR_PIPE_BUSY;

            const PIPE_AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(5);

            let attempt_start = Instant::now();
            let client = loop {
                match named_pipe::ClientOptions::new()
                    .read(true)
                    .write(true)
                    .open(name.as_str())
                {
                    Ok(client) => break client,
                    Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                        if attempt_start.elapsed() < PIPE_AVAILABILITY_TIMEOUT {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            continue;
                        } else {
                            return Err(e.into());
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            };

            Ok(client)
        }
    }
}