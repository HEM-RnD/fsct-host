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

//! IPC server using unix sockets or windows pipes transport and JSON-RPC 2.0 / NDJSON framing.

#[cfg(unix)]
use crate::ports::unix::ipc_transport as transport;
#[cfg(windows)]
use crate::ports::windows::ipc_transport as transport;

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

#[cfg(unix)]
use std::os::fd::{AsRawFd, OwnedFd};

use anyhow::Context;
use futures::StreamExt;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::select;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use futures::SinkExt;
use uuid::Uuid;

use fsct::definitions::{DeviceInfo, FsctStatus, FsctTextMetadata, TimelineInfo};
use fsct::player_state::{PlayerState, TrackMetadata};
use fsct::{DeviceChangeEvent, FsctDriver, FSCT_PROTOCOL_VERSION};
use fsct_ipc::{
    RpcNotification, RpcRequest, RpcResponse,
    ERR_APPLICATION, ERR_METHOD_NOT_FOUND, MAX_LINE_BYTES,
};

use crate::joinable_task::{spawn_service, JoinableTaskHandle, MultiJoinableTaskHandle};

// ---------------------------------------------------------------------------
// Wire-format helpers for types that use std::time (not directly serializable)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct TimelineWire {
    position_ms: u64,
    update_unix_ms: i64,
    duration_ms: u64,
    rate: f64,
}

impl From<&TimelineInfo> for TimelineWire {
    fn from(t: &TimelineInfo) -> Self {
        let update_unix_ms = t.update_time
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(|e| -(e.duration().as_millis() as i64));
        Self {
            position_ms: t.position.as_millis() as u64,
            update_unix_ms,
            duration_ms: t.duration.as_millis() as u64,
            rate: t.rate,
        }
    }
}

impl TryFrom<TimelineWire> for TimelineInfo {
    type Error = anyhow::Error;
    fn try_from(w: TimelineWire) -> Result<Self, Self::Error> {
        let update_time = if w.update_unix_ms >= 0 {
            UNIX_EPOCH + Duration::from_millis(w.update_unix_ms as u64)
        } else {
            UNIX_EPOCH - Duration::from_millis((-w.update_unix_ms) as u64)
        };
        Ok(TimelineInfo {
            position: Duration::from_millis(w.position_ms),
            update_time,
            duration: Duration::from_millis(w.duration_ms),
            rate: w.rate,
        })
    }
}

// ---------------------------------------------------------------------------
// Endpoint types
// ---------------------------------------------------------------------------

enum EndpointDefinitionType {
    Path(String),
    #[cfg(unix)]
    Fd(Option<OwnedFd>),
}

/// IPC server that exposes FsctDriver API over a local IPC connection.
pub struct IpcServer {
    endpoint: EndpointDefinitionType,
    driver: Arc<dyn FsctDriver>,
    connections: Arc<Mutex<MultiJoinableTaskHandle>>,
}

impl IpcServer {
    pub fn with_socket_path(driver: Arc<dyn FsctDriver>, endpoint: &str) -> Self {
        Self {
            endpoint: EndpointDefinitionType::Path(endpoint.into()),
            driver,
            connections: Arc::new(Mutex::new(MultiJoinableTaskHandle::new())),
        }
    }

    #[cfg(unix)]
    pub fn with_socket_fd(driver: Arc<dyn FsctDriver>, endpoint: OwnedFd) -> Self {
        Self {
            endpoint: EndpointDefinitionType::Fd(Some(endpoint)),
            driver,
            connections: Arc::new(Mutex::new(MultiJoinableTaskHandle::new())),
        }
    }

    pub async fn serve(&mut self) -> anyhow::Result<()> {
        let listener = self.init_listener().await?;
        let incoming = listener.listen()?;
        tokio::pin!(incoming);
        loop {
            match incoming.as_mut().next().await {
                Some(Ok(stream)) => {
                    let handle = self.start_connection_service(stream);
                    self.connections.lock().unwrap().add(handle);
                }
                Some(Err(e)) => {
                    error!("IPC accept failed: {}", e);
                    return Err(anyhow::anyhow!("IPC accept failed: {}", e));
                }
                None => {
                    info!("IPC accept loop terminated");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn init_listener(&mut self) -> anyhow::Result<transport::EndpointListener> {
        match &mut self.endpoint {
            EndpointDefinitionType::Path(path) => {
                info!("FSCT IPC server listening on: {}", path);
                transport::EndpointListener::from_path(path.clone())
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to start IPC endpoint: {e}"))
            }
            #[cfg(unix)]
            EndpointDefinitionType::Fd(fd) => {
                let fd = fd.take().expect("IPC server already initialized with a socket fd");
                info!("FSCT IPC server listening on fd: {}", fd.as_raw_fd());
                transport::EndpointListener::from_fd(fd)
                    .map_err(|e| anyhow::anyhow!("Failed to start IPC endpoint: {e}"))
            }
        }
    }

    pub async fn shutdown(&self) {
        let connections = std::mem::take(self.connections.lock().unwrap().deref_mut());
        if let Err(e) = connections.shutdown().await {
            warn!("Some IPC connection service failed to join on shutdown: {}", e);
        }
        #[cfg(unix)]
        if let EndpointDefinitionType::Path(path) = &self.endpoint {
            let _ = tokio::fs::remove_file(path.as_str()).await;
        }
    }

    fn start_connection_service(
        &self,
        stream: impl AsyncRead + AsyncWrite + Send + Unpin + 'static,
    ) -> JoinableTaskHandle {
        let driver = self.driver.clone();
        spawn_service(move |mut stop| async move {
            let connection_id = Uuid::new_v4();
            info!("New IPC client connected, id = {}", connection_id);

            let conn_players: Arc<Mutex<HashSet<NonZeroU32>>> =
                Arc::new(Mutex::new(HashSet::new()));

            let (read_half, write_half) = tokio::io::split(stream);
            let reader = FramedRead::new(read_half, LinesCodec::new_with_max_length(MAX_LINE_BYTES));
            let writer = Arc::new(tokio::sync::Mutex::new(FramedWrite::new(
                write_half,
                LinesCodec::new_with_max_length(MAX_LINE_BYTES),
            )));

            // Spawn notification forwarder
            let notify_writer = writer.clone();
            let notify_driver = driver.clone();
            let notify_task = tokio::spawn(async move {
                match notify_driver.subscribe_device_changes().await {
                    Err(e) => warn!("subscribe_device_changes failed: {}", e),
                    Ok(mut rx) => {
                        loop {
                            match rx.recv().await {
                                Ok(event) => {
                                    let notif = device_event_to_notification(event);
                                    match serde_json::to_string(&notif) {
                                        Ok(line) => {
                                            let mut w = notify_writer.lock().await;
                                            if let Err(e) = w.send(line).await {
                                                warn!("notification send error: {}", e);
                                                break;
                                            }
                                        }
                                        Err(e) => warn!("notification serialize error: {}", e),
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                    warn!("device change receiver lagged by {} events", n);
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }
            });

            let handler = ConnectionHandler {
                driver: driver.clone(),
                conn_players: conn_players.clone(),
                connection_id,
            };

            tokio::pin!(reader);
            select! {
                _ = async {
                    while let Some(line_result) = reader.next().await {
                        match line_result {
                            Err(e) => {
                                warn!("IPC read error (id={}): {}", connection_id, e);
                                break;
                            }
                            Ok(line) => {
                                let response = handler.handle_line(&line).await;
                                match serde_json::to_string(&response) {
                                    Ok(resp_line) => {
                                        let mut w = writer.lock().await;
                                        if let Err(e) = w.send(resp_line).await {
                                            warn!("IPC write error (id={}): {}", connection_id, e);
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize response: {}", e);
                                    }
                                }
                            }
                        }
                    }
                } => {}
                _ = stop.signaled() => {
                    info!("IPC connection stop requested, id = {}", connection_id);
                }
            }

            notify_task.abort();

            // Auto-unregister all players from this connection
            let ids = std::mem::take(conn_players.lock().unwrap().deref_mut());
            for pid in ids {
                if let Err(e) = driver.unregister_player(pid).await {
                    warn!("auto-unregister_player failed for {}: {}", pid, e);
                }
            }
            info!("IPC connection closed, id = {}", connection_id);
        })
    }
}

fn device_event_to_notification(event: DeviceChangeEvent) -> RpcNotification {
    let (event_str, device_id) = match event {
        DeviceChangeEvent::Added(id) => ("added", id),
        DeviceChangeEvent::Removed(id) => ("removed", id),
    };
    RpcNotification::new(
        "device_changed",
        json!({ "event": event_str, "device_id": device_id.to_string() }),
    )
}

fn run_ipc_server(mut server: IpcServer) -> JoinableTaskHandle {
    spawn_service(move |mut stop| async move {
        select!(
            res = server.serve() => {
                if let Err(e) = res {
                    error!("IPC server terminated with error: {}", e);
                }
            }
            _ = stop.signaled() => {}
        );
        server.shutdown().await;
        info!("IPC server stopped");
    })
}

pub fn run_ipc_server_with_endpoint_path(driver: Arc<dyn FsctDriver>, endpoint: String) -> JoinableTaskHandle {
    run_ipc_server(IpcServer::with_socket_path(driver, &endpoint))
}

#[cfg(unix)]
pub fn run_ipc_server_with_fd(driver: Arc<dyn FsctDriver>, fd: OwnedFd) -> JoinableTaskHandle {
    run_ipc_server(IpcServer::with_socket_fd(driver, fd))
}

#[cfg(not(unix))]
pub fn run_ipc_server_with_fd(_driver: Arc<dyn FsctDriver>, _fd: i32) -> JoinableTaskHandle {
    panic!("IPC running from file descriptor not supported on this platform");
}

// ---------------------------------------------------------------------------
// Per-connection request handler
// ---------------------------------------------------------------------------

struct ConnectionHandler {
    driver: Arc<dyn FsctDriver>,
    conn_players: Arc<Mutex<HashSet<NonZeroU32>>>,
    connection_id: Uuid,
}

impl ConnectionHandler {
    async fn handle_line(&self, line: &str) -> RpcResponse {
        let req: RpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return RpcResponse::err(
                    JsonValue::Null,
                    fsct_ipc::ERR_PARSE_ERROR,
                    format!("parse error: {}", e),
                );
            }
        };
        let id = req.id.clone();
        match self.dispatch(&req.method, &req.params).await {
            Ok(result) => RpcResponse::ok(id, result),
            Err(e) => {
                let code = if e.to_string().starts_with("unknown method") {
                    ERR_METHOD_NOT_FOUND
                } else {
                    ERR_APPLICATION
                };
                RpcResponse::err(id, code, e.to_string())
            }
        }
    }

    async fn dispatch(&self, method: &str, params: &JsonValue) -> anyhow::Result<JsonValue> {
        match method {
            "get_protocol_version" => self.req_get_protocol_version(params).await,
            "register_player" => self.req_register_player(params).await,
            "unregister_player" => self.req_unregister_player(params).await,
            "assign_player_to_device" => self.req_assign_player_to_device(params).await,
            "unassign_player_from_device" => self.req_unassign_player_from_device(params).await,
            "update_player_state" => self.req_update_player_state(params).await,
            "update_player_status" => self.req_update_player_status(params).await,
            "update_player_timeline" => self.req_update_player_timeline(params).await,
            "update_player_metadata" => self.req_update_player_metadata(params).await,
            "get_player_assigned_device" => self.req_get_player_assigned_device(params).await,
            "get_detected_devices" => self.req_get_detected_devices(params).await,
            "get_device_info" => self.req_get_device_info(params).await,
            _ => Err(anyhow::anyhow!("unknown method: {}", method)),
        }
    }

    // -----------------------------------------------------------------------
    // Parameter helpers
    // -----------------------------------------------------------------------

    fn ensure_player_for_connection(&self, pid: NonZeroU32) -> anyhow::Result<()> {
        if self.conn_players.lock().unwrap().contains(&pid) {
            Ok(())
        } else {
            Err(anyhow::anyhow!("player_id {} not registered for this connection", pid))
        }
    }

    fn parse_player_id(params: &JsonValue) -> anyhow::Result<NonZeroU32> {
        let v = &params["player_id"];
        let n = v.as_u64().with_context(|| "player_id must be a non-zero integer")?;
        NonZeroU32::new(n as u32).with_context(|| "player_id must be non-zero")
    }

    fn parse_device_id(params: &JsonValue) -> anyhow::Result<Uuid> {
        let s = params["device_id"].as_str()
            .with_context(|| "device_id must be a UUID string")?;
        Uuid::parse_str(s).with_context(|| format!("invalid device_id UUID: {}", s))
    }

    fn parse_status(params: &JsonValue) -> anyhow::Result<FsctStatus> {
        serde_json::from_value(params["status"].clone())
            .with_context(|| "invalid status value")
    }

    fn parse_text_metadata_id(params: &JsonValue) -> anyhow::Result<FsctTextMetadata> {
        serde_json::from_value(params["metadata_id"].clone())
            .with_context(|| "invalid metadata_id value")
    }

    fn parse_timeline_opt(v: &JsonValue) -> anyhow::Result<Option<TimelineInfo>> {
        if v.is_null() {
            return Ok(None);
        }
        let wire: TimelineWire = serde_json::from_value(v.clone())
            .with_context(|| "invalid timeline object")?;
        Ok(Some(wire.try_into()?))
    }

    fn parse_player_state(params: &JsonValue) -> anyhow::Result<PlayerState> {
        let state_val = &params["state"];
        if !state_val.is_object() {
            anyhow::bail!("state must be an object");
        }

        // Validate no unknown keys
        if let Some(obj) = state_val.as_object() {
            for key in obj.keys() {
                match key.as_str() {
                    "status" | "timeline" | "texts" => {}
                    other => anyhow::bail!("invalid player state key: {}", other),
                }
            }

            // Validate texts sub-object if present
            if let Some(texts_val) = obj.get("texts") {
                if !texts_val.is_object() {
                    anyhow::bail!("texts must be an object");
                }
                if let Some(texts_obj) = texts_val.as_object() {
                    for key in texts_obj.keys() {
                        match key.as_str() {
                            "title" | "artist" | "album" | "genre" => {
                                let v = &texts_obj[key];
                                if !v.is_null() && !v.is_string() {
                                    anyhow::bail!("text '{}' must be string or null", key);
                                }
                            }
                            other => anyhow::bail!("invalid text key: {}", other),
                        }
                    }
                }
            }

            // Validate status if present
            if let Some(status_val) = obj.get("status") {
                serde_json::from_value::<FsctStatus>(status_val.clone())
                    .with_context(|| "invalid status value")?;
            }

            // Validate timeline if present
            if let Some(timeline_val) = obj.get("timeline") {
                Self::parse_timeline_opt(timeline_val)?;
            }
        }

        let status: FsctStatus = if state_val["status"].is_null() || state_val.get("status").is_none() {
            FsctStatus::default()
        } else {
            serde_json::from_value(state_val["status"].clone())
                .with_context(|| "invalid status value")?
        };

        let timeline = Self::parse_timeline_opt(&state_val["timeline"])?;

        let texts = if let Some(texts_val) = state_val.get("texts") {
            serde_json::from_value(texts_val.clone())
                .with_context(|| "invalid texts object")?
        } else {
            TrackMetadata::default()
        };

        Ok(PlayerState { status, timeline, texts })
    }

    // -----------------------------------------------------------------------
    // Method handlers
    // -----------------------------------------------------------------------

    async fn req_get_protocol_version(&self, _params: &JsonValue) -> anyhow::Result<JsonValue> {
        Ok(serde_json::to_value(FSCT_PROTOCOL_VERSION)?)
    }

    async fn req_register_player(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let self_id = params["self_id"].as_str()
            .with_context(|| "self_id must be a string")?
            .to_string();
        let pid = self.driver.register_player(self_id).await
            .with_context(|| "register_player error")?;
        {
            let mut guard = self.conn_players.lock().unwrap();
            guard.insert(pid);
        }
        info!("Registered player {} for connection {}", pid.get(), self.connection_id);
        Ok(json!(pid.get()))
    }

    async fn req_unregister_player(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        self.driver.unregister_player(pid).await
            .with_context(|| "unregister_player error")?;
        {
            let mut guard = self.conn_players.lock().unwrap();
            guard.remove(&pid);
        }
        info!("Unregistered player {} for connection {}", pid.get(), self.connection_id);
        Ok(JsonValue::Null)
    }

    async fn req_assign_player_to_device(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        let did = Self::parse_device_id(params)?;
        self.driver.assign_player_to_device(pid, did).await?;
        Ok(JsonValue::Null)
    }

    async fn req_unassign_player_from_device(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        let did = Self::parse_device_id(params)?;
        self.driver.unassign_player_from_device(pid, did).await?;
        Ok(JsonValue::Null)
    }

    async fn req_update_player_state(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        let state = Self::parse_player_state(params)?;
        self.driver.update_player_state(pid, state).await?;
        Ok(JsonValue::Null)
    }

    async fn req_update_player_status(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        let status = Self::parse_status(params)?;
        self.driver.update_player_status(pid, status).await?;
        Ok(JsonValue::Null)
    }

    async fn req_update_player_timeline(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        let timeline = Self::parse_timeline_opt(&params["timeline"])?;
        self.driver.update_player_timeline(pid, timeline).await?;
        Ok(JsonValue::Null)
    }

    async fn req_update_player_metadata(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        let meta = Self::parse_text_metadata_id(params)?;
        let text = match &params["text"] {
            JsonValue::Null => None,
            JsonValue::String(s) => Some(s.clone()),
            _ => anyhow::bail!("text must be string or null"),
        };
        self.driver.update_player_metadata(pid, meta, text).await?;
        Ok(JsonValue::Null)
    }

    async fn req_get_player_assigned_device(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let pid = Self::parse_player_id(params)?;
        self.ensure_player_for_connection(pid)?;
        let opt = self.driver.get_player_assigned_device(pid).await?;
        Ok(match opt {
            Some(uuid) => json!(uuid.to_string()),
            None => JsonValue::Null,
        })
    }

    async fn req_get_detected_devices(&self, _params: &JsonValue) -> anyhow::Result<JsonValue> {
        let ids = self.driver.get_detected_devices().await?;
        let arr: Vec<JsonValue> = ids.iter().map(|id| json!(id.to_string())).collect();
        Ok(json!(arr))
    }

    async fn req_get_device_info(&self, params: &JsonValue) -> anyhow::Result<JsonValue> {
        let did = Self::parse_device_id(params)?;
        let info: DeviceInfo = self.driver.get_device_info(did).await?;
        Ok(serde_json::to_value(info)?)
    }
}
