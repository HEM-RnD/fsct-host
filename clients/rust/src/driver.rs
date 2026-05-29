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

use anyhow::Context;
use async_trait::async_trait;
use fsct::definitions::{
    DeviceInfo, FsctStatus, FsctTextMetadata, ManagedDeviceId, ManagedPlayerId, ProtocolVersion, TimeSync, TimelineInfo,
};
use fsct::player_state::PlayerState;
use fsct::{
    DeviceChangeEvent, FsctDriver, default_endpoint_path, instant_from_mono_ns, mono_ns_of, mono_offset_ns,
    offsets_consistent,
};
use serde_json::{Value as JsonValue, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use crate::mux::{OutboundCall, run_mux};
use crate::rpc::MAX_LINE_BYTES;

/// Maximum tolerated disagreement (ns) between two handshake offset samples before we retry.
/// A few milliseconds easily covers scheduling jitter while still catching a real wall-clock step.
const OFFSET_CONSISTENCY_TOLERANCE_NS: i128 = 5_000_000;
/// How many times to retry the two-sample handshake before giving up.
const OFFSET_HANDSHAKE_ATTEMPTS: usize = 5;

pub struct IpcDriver {
    call_tx: mpsc::Sender<OutboundCall>,
    device_tx: broadcast::Sender<DeviceChangeEvent>,
    negotiated_version: ProtocolVersion,
    /// Offset (ns) converting this client's monotonic frame to the driver's: `driver = client + offset`.
    mono_offset_ns: i128,
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

        let task = tokio::spawn(run_mux(reader, writer, call_rx, device_tx_task));

        let negotiated_version = Self::perform_handshake(&call_tx).await.inspect_err(|_| task.abort())?;
        let mono_offset_ns = Self::establish_mono_offset(&call_tx)
            .await
            .inspect_err(|_| task.abort())?;

        Ok(Self {
            call_tx,
            device_tx,
            negotiated_version,
            mono_offset_ns,
            _task: task,
        })
    }

    async fn perform_handshake(call_tx: &mpsc::Sender<OutboundCall>) -> anyhow::Result<ProtocolVersion> {
        let (reply_tx, reply_rx) = oneshot::channel();
        call_tx
            .send(OutboundCall {
                method: "get_protocol_version".into(),
                params: json!({}),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("IPC connection closed"))?;
        let resp = reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("IPC connection closed"))?
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let major = resp["major"]
            .as_u64()
            .with_context(|| "missing major in protocol version")? as u16;
        let minor = resp["minor"]
            .as_u64()
            .with_context(|| "missing minor in protocol version")? as u16;
        let negotiated = ProtocolVersion { major, minor };

        if negotiated.major != fsct::FSCT_PROTOCOL_VERSION.major {
            return Err(anyhow::anyhow!(
                "incompatible protocol version: remote {}.{} != local {}.{}",
                major,
                minor,
                fsct::FSCT_PROTOCOL_VERSION.major,
                fsct::FSCT_PROTOCOL_VERSION.minor
            ));
        }
        Ok(negotiated)
    }

    /// Establish the monotonic-frame offset to the driver via a mini NTP/PTP handshake.
    ///
    /// Each round-trip fetches the driver's `(wall, mono)` sample and pairs it with a local
    /// `(wall, mono)` sample taken right after the response arrives. Two round-trips are compared:
    /// if they agree (no wall step occurred in between) the averaged offset is returned, otherwise
    /// we retry. This runs once per connection, so a driver restart (which yields a new monotonic
    /// epoch) is re-bridged automatically on reconnect.
    async fn establish_mono_offset(call_tx: &mpsc::Sender<OutboundCall>) -> anyhow::Result<i128> {
        let mut last_pair: Option<(i128, i128)> = None;
        for _ in 0..OFFSET_HANDSHAKE_ATTEMPTS {
            let k1 = Self::measure_offset_once(call_tx).await?;
            let k2 = Self::measure_offset_once(call_tx).await?;
            if offsets_consistent(k1, k2, OFFSET_CONSISTENCY_TOLERANCE_NS) {
                return Ok((k1 + k2) / 2);
            }
            last_pair = Some((k1, k2));
        }
        Err(anyhow::anyhow!(
            "time-sync handshake did not stabilize after {} attempts (last samples: {:?}); \
             the wall clock may be stepping repeatedly",
            OFFSET_HANDSHAKE_ATTEMPTS,
            last_pair
        ))
    }

    async fn measure_offset_once(call_tx: &mpsc::Sender<OutboundCall>) -> anyhow::Result<i128> {
        let driver = Self::fetch_timesync(call_tx).await?;
        let client = TimeSync::sample_now();
        Ok(mono_offset_ns(
            driver.wall_ns as i128,
            driver.mono_ns as i128,
            client.wall_ns as i128,
            client.mono_ns as i128,
        ))
    }

    async fn fetch_timesync(call_tx: &mpsc::Sender<OutboundCall>) -> anyhow::Result<TimeSync> {
        let (reply_tx, reply_rx) = oneshot::channel();
        call_tx
            .send(OutboundCall {
                method: "get_timesync".into(),
                params: json!({}),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("IPC connection closed"))?;
        let resp = reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("IPC connection closed"))?
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        serde_json::from_value(resp).context("invalid get_timesync response")
    }

    /// Shift a timeline's monotonic anchor from this client's frame into the driver's frame, so
    /// the driver can compare it directly against its own clock. Applied to the typed value
    /// before serialization, so the wire format naturally carries a driver-frame stamp without
    /// any post-serialization patching.
    fn anchor_in_driver_frame(&self, mut timeline: TimelineInfo) -> TimelineInfo {
        let client_ns = mono_ns_of(timeline.update_time) as i128;
        let driver_ns = (client_ns + self.mono_offset_ns).max(0) as u64;
        timeline.update_time = instant_from_mono_ns(driver_ns);
        timeline
    }

    async fn rpc_call(&self, method: &str, params: JsonValue) -> anyhow::Result<JsonValue> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.call_tx
            .send(OutboundCall {
                method: method.into(),
                params,
                reply: reply_tx,
            })
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
        let id = resp
            .as_u64()
            .with_context(|| "invalid response for register_player: expected integer")? as u32;
        std::num::NonZeroU32::new(id).with_context(|| "server returned zero player id")
    }

    async fn unregister_player(&self, player_id: ManagedPlayerId) -> anyhow::Result<()> {
        self.rpc_call("unregister_player", json!({ "player_id": player_id.get() }))
            .await?;
        Ok(())
    }

    async fn assign_player_to_device(
        &self,
        player_id: ManagedPlayerId,
        device_id: ManagedDeviceId,
    ) -> anyhow::Result<()> {
        self.rpc_call(
            "assign_player_to_device",
            json!({ "player_id": player_id.get(), "device_id": device_id.to_string() }),
        )
        .await?;
        Ok(())
    }

    async fn unassign_player_from_device(
        &self,
        player_id: ManagedPlayerId,
        device_id: ManagedDeviceId,
    ) -> anyhow::Result<()> {
        self.rpc_call(
            "unassign_player_from_device",
            json!({ "player_id": player_id.get(), "device_id": device_id.to_string() }),
        )
        .await?;
        Ok(())
    }

    async fn update_player_state(&self, player_id: ManagedPlayerId, mut new_state: PlayerState) -> anyhow::Result<()> {
        new_state.timeline = new_state.timeline.map(|t| self.anchor_in_driver_frame(t));
        self.rpc_call(
            "update_player_state",
            json!({ "player_id": player_id.get(), "state": serde_json::to_value(&new_state)? }),
        )
        .await?;
        Ok(())
    }

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> anyhow::Result<()> {
        self.rpc_call(
            "update_player_status",
            json!({ "player_id": player_id.get(), "status": new_status }),
        )
        .await?;
        Ok(())
    }

    async fn update_player_timeline(
        &self,
        player_id: ManagedPlayerId,
        new_timeline: Option<TimelineInfo>,
    ) -> anyhow::Result<()> {
        let timeline = new_timeline.map(|t| self.anchor_in_driver_frame(t));
        self.rpc_call(
            "update_player_timeline",
            json!({ "player_id": player_id.get(), "timeline": timeline }),
        )
        .await?;
        Ok(())
    }

    async fn update_player_metadata(
        &self,
        player_id: ManagedPlayerId,
        metadata_id: FsctTextMetadata,
        new_text: Option<String>,
    ) -> anyhow::Result<()> {
        self.rpc_call(
            "update_player_metadata",
            json!({ "player_id": player_id.get(), "metadata_id": metadata_id, "text": new_text }),
        )
        .await?;
        Ok(())
    }

    async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> anyhow::Result<Option<ManagedDeviceId>> {
        let resp = self
            .rpc_call("get_player_assigned_device", json!({ "player_id": player_id.get() }))
            .await?;
        if resp.is_null() {
            return Ok(None);
        }
        let s = resp
            .as_str()
            .with_context(|| "get_player_assigned_device: expected UUID string or null")?;
        Ok(Some(uuid::Uuid::parse_str(s)?))
    }

    async fn get_detected_devices(&self) -> anyhow::Result<Vec<ManagedDeviceId>> {
        let resp = self.rpc_call("get_detected_devices", json!({})).await?;
        let arr = resp
            .as_array()
            .with_context(|| "get_detected_devices: expected array")?;
        arr.iter()
            .map(|v| {
                let s = v.as_str().with_context(|| "device id must be a string")?;
                uuid::Uuid::parse_str(s).with_context(|| format!("invalid UUID: {}", s))
            })
            .collect()
    }

    async fn get_device_info(&self, device_id: ManagedDeviceId) -> anyhow::Result<DeviceInfo> {
        let resp = self
            .rpc_call("get_device_info", json!({ "device_id": device_id.to_string() }))
            .await?;
        serde_json::from_value(resp).with_context(|| "failed to deserialize DeviceInfo")
    }

    async fn subscribe_device_changes(&self) -> anyhow::Result<broadcast::Receiver<DeviceChangeEvent>> {
        Ok(self.device_tx.subscribe())
    }

    async fn get_timesync(&self) -> anyhow::Result<TimeSync> {
        let resp = self.rpc_call("get_timesync", json!({})).await?;
        serde_json::from_value(resp).context("invalid get_timesync response")
    }
}

mod transport {
    pub struct EndpointClient;

    impl EndpointClient {
        #[cfg(unix)]
        pub async fn connect(path: String) -> anyhow::Result<tokio::net::UnixStream> {
            use anyhow::Context;
            tokio::net::UnixStream::connect(path)
                .await
                .context("unix client connect failed")
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
