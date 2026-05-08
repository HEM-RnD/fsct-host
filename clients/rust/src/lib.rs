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

pub mod rpc;

use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
use fsct::definitions::{DeviceInfo, FsctStatus, FsctTextMetadata, ManagedDeviceId, ManagedPlayerId, ProtocolVersion, TimelineInfo};
use fsct::player_state::PlayerState;
use fsct::{default_endpoint_path, DeviceChangeEvent, FsctDriver};
use log::warn;
use serde_json::{json, Value as JsonValue};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use futures::{SinkExt, StreamExt};
use crate::rpc::{RpcNotification, RpcRequest, RpcResponse, MAX_LINE_BYTES};

// ---------------------------------------------------------------------------
// Multiplexer background task
// ---------------------------------------------------------------------------

struct OutboundCall {
    method: String,
    params: JsonValue,
    reply: oneshot::Sender<Result<JsonValue, String>>,
}

async fn run_mux<R, W>(
    mut reader: FramedRead<R, LinesCodec>,
    mut writer: FramedWrite<W, LinesCodec>,
    mut rx: mpsc::Receiver<OutboundCall>,
    device_tx: broadcast::Sender<DeviceChangeEvent>,
)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut pending: HashMap<u64, oneshot::Sender<Result<JsonValue, String>>> = HashMap::new();
    let mut next_id: u64 = 1;

    loop {
        tokio::select! {
            // Outbound: caller wants to send a request
            call = rx.recv() => {
                let Some(call) = call else { break };
                let id = next_id;
                next_id += 1;
                let req = RpcRequest {
                    jsonrpc: "2.0".into(),
                    id: json!(id),
                    method: call.method,
                    params: call.params,
                };
                match serde_json::to_string(&req) {
                    Ok(line) => {
                        if writer.send(line).await.is_err() {
                            let _ = call.reply.send(Err("connection closed".into()));
                            break;
                        }
                        pending.insert(id, call.reply);
                    }
                    Err(e) => {
                        let _ = call.reply.send(Err(format!("serialize error: {}", e)));
                    }
                }
            }
            // Inbound: a line arrived from the server
            line = reader.next() => {
                let Some(line) = line else {
                    // Connection closed — fail all pending
                    for (_, tx) in pending.drain() {
                        let _ = tx.send(Err("connection closed".into()));
                    }
                    break;
                };
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        warn!("IPC read error: {}", e);
                        for (_, tx) in pending.drain() {
                            let _ = tx.send(Err(format!("read error: {}", e)));
                        }
                        break;
                    }
                };
                // Try to parse as response (has "id"), else as notification
                if let Ok(resp) = serde_json::from_str::<RpcResponse>(&line) {
                    if let Some(id_u64) = resp.id.as_u64() {
                        if let Some(tx) = pending.remove(&id_u64) {
                            let result = if let Some(err) = resp.error {
                                Err(err.message)
                            } else {
                                Ok(resp.result.unwrap_or(JsonValue::Null))
                            };
                            let _ = tx.send(result);
                        }
                    }
                } else if let Ok(notif) = serde_json::from_str::<RpcNotification>(&line) {
                    handle_notification(&notif, &device_tx);
                } else {
                    warn!("IPC: unrecognized message: {}", &line[..line.len().min(200)]);
                }
            }
        }
    }
}

fn handle_notification(notif: &RpcNotification, tx: &broadcast::Sender<DeviceChangeEvent>) {
    if notif.method != "device_changed" {
        warn!("unknown notification: {}", notif.method);
        return;
    }
    let event_str = notif.params["event"].as_str().unwrap_or("");
    let device_id_str = notif.params["device_id"].as_str().unwrap_or("");
    let uuid = match uuid::Uuid::parse_str(device_id_str) {
        Ok(u) => u,
        Err(_) => {
            warn!("invalid device_id in notification: {}", device_id_str);
            return;
        }
    };
    let event = match event_str {
        "added" => DeviceChangeEvent::Added(uuid),
        "removed" => DeviceChangeEvent::Removed(uuid),
        _ => {
            warn!("unknown device_changed event: {}", event_str);
            return;
        }
    };
    let _ = tx.send(event);
}

// ---------------------------------------------------------------------------
// IpcDriver
// ---------------------------------------------------------------------------

pub struct IpcDriver {
    call_tx: mpsc::Sender<OutboundCall>,
    device_tx: broadcast::Sender<DeviceChangeEvent>,
    negotiated_version: ProtocolVersion,
    _task: tokio::task::JoinHandle<()>,
}

impl Drop for IpcDriver {
    fn drop(&mut self) {
        self._task.abort();
    }
}

impl IpcDriver {
    pub async fn connect() -> anyhow::Result<Self> {
        Self::connect_to_endpoint(default_endpoint_path().to_string()).await
    }

    pub async fn connect_to_endpoint(endpoint: String) -> anyhow::Result<Self> {
        let stream = transport::EndpointClient::connect(endpoint)
            .await
            .map_err(|e| anyhow::anyhow!("IPC connect error: {e}"))?;
        Self::from_stream(stream).await
    }

    async fn from_stream<S>(stream: S) -> anyhow::Result<Self>
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let (read_half, write_half) = tokio::io::split(stream);
        let reader = FramedRead::new(read_half, LinesCodec::new_with_max_length(MAX_LINE_BYTES));
        let writer = FramedWrite::new(write_half, LinesCodec::new_with_max_length(MAX_LINE_BYTES));

        let (call_tx, call_rx) = mpsc::channel::<OutboundCall>(64);
        let (device_tx, _) = broadcast::channel::<DeviceChangeEvent>(100);
        let device_tx_task = device_tx.clone();

        let task = tokio::spawn(async move {
            run_mux(reader, writer, call_rx, device_tx_task).await;
        });

        // Handshake: send get_protocol_version before constructing Self (avoids Drop conflicts)
        let (reply_tx, reply_rx) = oneshot::channel();
        call_tx.send(OutboundCall {
            method: "get_protocol_version".into(),
            params: json!({}),
            reply: reply_tx,
        }).await.map_err(|_| { task.abort(); anyhow::anyhow!("IPC connection closed") })?;
        let resp = reply_rx.await
            .map_err(|_| { task.abort(); anyhow::anyhow!("IPC connection closed") })?
            .map_err(|e| { task.abort(); anyhow::anyhow!("{}", e) })?;

        let major = resp["major"].as_u64()
            .with_context(|| "missing major in protocol version")? as u16;
        let minor = resp["minor"].as_u64()
            .with_context(|| "missing minor in protocol version")? as u16;
        let negotiated_version = ProtocolVersion { major, minor };

        if negotiated_version.major != fsct::FSCT_PROTOCOL_VERSION.major {
            task.abort();
            return Err(anyhow::anyhow!(
                "incompatible protocol version: remote {}.{} != local {}.{}",
                major, minor,
                fsct::FSCT_PROTOCOL_VERSION.major,
                fsct::FSCT_PROTOCOL_VERSION.minor
            ));
        }

        Ok(Self { call_tx, device_tx, negotiated_version, _task: task })
    }

    async fn rpc_call(&self, method: &str, params: JsonValue) -> anyhow::Result<JsonValue> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.call_tx
            .send(OutboundCall { method: method.into(), params, reply: reply_tx })
            .await
            .map_err(|_| anyhow::anyhow!("IPC connection closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("IPC connection closed"))?
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    pub async fn get_protocol_version(&self) -> anyhow::Result<ProtocolVersion> {
        Ok(self.negotiated_version)
    }
}

#[async_trait]
impl FsctDriver for IpcDriver {
    async fn register_player(&self, self_id: String) -> anyhow::Result<ManagedPlayerId> {
        let resp = self.rpc_call("register_player", json!({ "self_id": self_id })).await?;
        let id = resp.as_u64()
            .with_context(|| "invalid response for register_player: expected integer")? as u32;
        std::num::NonZeroU32::new(id)
            .with_context(|| "server returned zero player id")
    }

    async fn unregister_player(&self, player_id: ManagedPlayerId) -> anyhow::Result<()> {
        self.rpc_call("unregister_player", json!({ "player_id": player_id.get() })).await?;
        Ok(())
    }

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<()> {
        self.rpc_call("assign_player_to_device", json!({
            "player_id": player_id.get(),
            "device_id": device_id.to_string(),
        })).await?;
        Ok(())
    }

    async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> anyhow::Result<()> {
        self.rpc_call("unassign_player_from_device", json!({
            "player_id": player_id.get(),
            "device_id": device_id.to_string(),
        })).await?;
        Ok(())
    }

    async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> anyhow::Result<()> {
        self.rpc_call("update_player_state", json!({
            "player_id": player_id.get(),
            "state": serde_json::to_value(&new_state)?,
        })).await?;
        Ok(())
    }

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> anyhow::Result<()> {
        self.rpc_call("update_player_status", json!({
            "player_id": player_id.get(),
            "status": new_status,
        })).await?;
        Ok(())
    }

    async fn update_player_timeline(&self, player_id: ManagedPlayerId, new_timeline: Option<TimelineInfo>) -> anyhow::Result<()> {
        self.rpc_call("update_player_timeline", json!({
            "player_id": player_id.get(),
            "timeline": serde_json::to_value(&new_timeline)?,
        })).await?;
        Ok(())
    }

    async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: Option<String>) -> anyhow::Result<()> {
        self.rpc_call("update_player_metadata", json!({
            "player_id": player_id.get(),
            "metadata_id": metadata_id,
            "text": new_text,
        })).await?;
        Ok(())
    }

    async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>> {
        let resp = self.rpc_call("get_player_assigned_device", json!({ "player_id": player_id.get() })).await?;
        if resp.is_null() { return Ok(None); }
        let s = resp.as_str()
            .with_context(|| "get_player_assigned_device: expected UUID string or null")?;
        Ok(Some(uuid::Uuid::parse_str(s)?))
    }

    async fn get_detected_devices(&self) -> anyhow::Result<Vec<ManagedDeviceId>> {
        let resp = self.rpc_call("get_detected_devices", json!({})).await?;
        let arr = resp.as_array()
            .with_context(|| "get_detected_devices: expected array")?;
        arr.iter()
            .map(|v| {
                let s = v.as_str().with_context(|| "device id must be a string")?;
                uuid::Uuid::parse_str(s).with_context(|| format!("invalid UUID: {}", s))
            })
            .collect()
    }

    async fn get_device_info(&self, device_id: ManagedDeviceId) -> anyhow::Result<DeviceInfo> {
        let resp = self.rpc_call("get_device_info", json!({ "device_id": device_id.to_string() })).await?;
        serde_json::from_value(resp).with_context(|| "failed to deserialize DeviceInfo")
    }

    async fn subscribe_device_changes(&self) -> anyhow::Result<broadcast::Receiver<DeviceChangeEvent>> {
        Ok(self.device_tx.subscribe())
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
