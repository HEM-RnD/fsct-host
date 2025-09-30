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

//! IPC server using unix sockets or windows pipes transport and MessagePack(-RPC style) framing.
//!
//! The server currently implements a minimal subset required by docs/ipc_plan.md phase 2:
//! - Accept connections on a local endpoint
//! - Handle msgpack-rpc style requests for `get_protocol_version`
//! - Forward to the provided FsctDriver


#[cfg(unix)]
use crate::ports::unix::ipc_transport as transport;
#[cfg(windows)]
use crate::ports::windows::ipc_transport as transport;

use std::sync::{Arc, Mutex};

use log::{error, info, warn};
use anyhow::{anyhow, bail, Context};

use fsct::joinable_task::{spawn_service, JoinableTaskHandle, MultiJoinableTaskHandle};
use fsct::player_state::{PlayerState, TrackMetadata};
use fsct::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use fsct::{FsctDriver, ProtocolVersion, FSCT_PROTOCOL_VERSION};

use uuid::Uuid;
use msgpack_rpc::{serve, Service, Value};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::select;
use tokio_util::compat::TokioAsyncReadCompatExt;
use futures::StreamExt;

#[cfg(unix)]
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, UNIX_EPOCH};
use std::num::NonZeroU32;
use std::future::Future;
use std::pin::Pin;
use std::collections::HashSet;
use std::ops::DerefMut;


enum EndpointDefinitionType {
    Path(String),
    #[cfg(unix)]
    Fd(Option<OwnedFd>),
}

/// IPC server that exposes FsctDriver API over a local IPC connection.
pub struct IpcServer {
    endpoint: EndpointDefinitionType,
    driver: Arc<dyn FsctDriver>,
    // Container of per-connection services for cooperative shutdown
    connections: Arc<Mutex<fsct::MultiJoinableTaskHandle>>,
}

impl IpcServer {
    /// Create with an explicit socket path (useful for tests).
    pub fn with_socket_path(driver: Arc<dyn FsctDriver>, endpoint: &str) -> Self {
        Self { endpoint: EndpointDefinitionType::Path(endpoint.into()), driver, connections: Arc::new(Mutex::new(MultiJoinableTaskHandle::new())) }
    }

    #[cfg(unix)]
    pub fn with_socket_fd(driver: Arc<dyn FsctDriver>, endpoint: OwnedFd) -> Self {
        Self { endpoint: EndpointDefinitionType::Fd(Some(endpoint)), driver, connections: Arc::new(Mutex::new(MultiJoinableTaskHandle::new())) }
    }

    /// Start serving and block until the accept loop terminates (e.g., due to unrecoverable error or shutdown signal via drop).
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
                    break
                },
            }
        }

        Ok(())
    }

    async fn init_listener(&mut self) -> anyhow::Result<transport::EndpointListener> {
        let listener = match &mut self.endpoint {
            EndpointDefinitionType::Path(path) => {
                info!("FSCT IPC server listening on: {}", path);
                let listener = transport::EndpointListener::from_path(path.clone()).await
                                                                                   .map_err(|e| anyhow::anyhow!("Failed to start IPC endpoint: {e}"))?;
                listener
            }
            #[cfg(unix)]
            EndpointDefinitionType::Fd(fd) => {
                let fd = fd.take().expect("IPC server already initialized with a socket fd");
                info!("FSCT IPC server listening on fd: {}", fd.as_raw_fd());
                let listener = transport::EndpointListener::from_fd(fd)
                    .map_err(|e| anyhow::anyhow!("Failed to start IPC endpoint: {e}"))?;
                listener
            }
        };
        Ok(listener)
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

    fn start_connection_service(&self,
                                stream: impl AsyncRead + AsyncWrite + Send + Unpin + 'static) -> JoinableTaskHandle {
        let driver = self.driver.clone();
        spawn_service(move |mut stop| async move {
            let connection_id = Uuid::new_v4();
            info!("New IPC client connected, id = {}", connection_id);
            let service = FsctRpcService::new(driver.clone(), connection_id);
            let players = service.conn_players_arc();
            let mut compat_stream = stream.compat();
            select! {
                res = serve(&mut compat_stream, service) => {
                    if let Err(e) = res { warn!("IPC connection (id = {}) handler ended with error: {}",
                        connection_id, e); }
                }
                _ = stop.signaled() => {
                    info!("IPC connection stop requested, id = {}", connection_id);
                    drop(compat_stream); // dropping compat_stream will close the connection
                }
            }
            // After either completion or stop: unregister all players tied to this connection
            let ids = std::mem::take(players.lock().unwrap().deref_mut());
            for pid in ids {
                if let Err(e) = driver.unregister_player(pid).await {
                    warn!("auto-unregister_player failed for {}: {}", pid, e);
                }
            }
            info!("IPC connection closed, id = {}", connection_id);
        })
    }
}


fn run_ipc_server(mut server: IpcServer) -> JoinableTaskHandle {
    spawn_service(move |mut stop| async move {
        // Reuse IpcServer::serve instead of duplicating accept-loop logic

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

/// Run the IPC server as a background service and return a ServiceHandle for cooperative shutdown.
pub fn run_ipc_server_with_endpoint_path(driver: Arc<dyn FsctDriver>, endpoint: String) -> JoinableTaskHandle {
    let server = IpcServer::with_socket_path(driver, &endpoint);
    run_ipc_server(server)
}

/// Run an IPC (Inter-Process Communication) server using a provided file descriptor.
#[cfg(unix)]
pub fn run_ipc_server_with_fd(driver: Arc<dyn FsctDriver>, fd: OwnedFd) -> JoinableTaskHandle {
    let server = IpcServer::with_socket_fd(driver, fd);
    return run_ipc_server(server);
}

#[cfg(not(unix))]
pub fn run_ipc_server_with_fd(_driver: Arc<dyn FsctDriver>, _fd: i32) -> JoinableTaskHandle {
    panic!("IPC running from file descriptor not supported on this platform");
}

#[derive(Clone)]
struct FsctRpcService {
    driver: Arc<dyn FsctDriver>,
    // Set of player IDs registered via this connection
    conn_players: Arc<Mutex<HashSet<NonZeroU32>>>,
    connection_id: uuid::Uuid,
}

// Request future type alias used by per-method handlers
type RequestFut = Pin<Box<dyn Future<Output=Result<Value, Value>> + Send>>;

// Small helper to produce an immediate error future without an async block
fn fut_err<V: Into<Value>>(v: V) -> RequestFut {
    Box::pin(std::future::ready(Err(v.into())))
}


trait IntoValue {
    fn into_value(self) -> Value;
}

impl IntoValue for ProtocolVersion {
    fn into_value(self) -> Value {
        Value::Map(vec![
            ("major".into(), self.major.into()),
            ("minor".into(), self.minor.into()),
        ])
    }
}

struct Nil;

impl Into<Value> for Nil {
    fn into(self) -> Value {
        Value::Nil
    }
}

fn parse_player_id(param: &Value) -> Result<std::num::NonZeroU32, anyhow::Error> {
    let pid_u64 = param.as_u64()
                       .with_context(|| "invalid param: player_id must be integer")?;
    let pid = std::num::NonZeroU32::new(pid_u64 as u32)
        .with_context(|| "invalid player_id: must be non-zero")?;
    Ok(pid)
}

fn parse_device_id(param: &Value) -> Result<Uuid, anyhow::Error> {
    let did_bytes = param.as_slice()
                         .with_context(|| "invalid param: device_id must be binary")?;
    if did_bytes.len() != 16 {
        bail!("invalid device_id: uuid binary must be 16 bytes");
    }
    let did = Uuid::from_slice(did_bytes)
        .with_context(|| "invalid device_id: must be valid 16-byte uuid")?;
    Ok(did)
}

fn expect_params(params: &[Value], expected_len: usize, expected_params_names: &str) -> Result<(), anyhow::Error> {
    if params.len() != expected_len {
        return Err(anyhow!("expected {} param{}{}{}",
                           expected_len,
                           if expected_len == 1 { "" } else { "s" },
                           if expected_params_names.is_empty() { "" } else { ": " },
                           expected_params_names));
    }
    Ok(())
}

impl FsctRpcService {
    fn new(driver: Arc<dyn FsctDriver>, connection_id: Uuid) -> Self {
        Self {
            driver,
            conn_players: Arc::new(Mutex::new(HashSet::new())),
            connection_id,
        }
    }
    fn conn_players_arc(&self) -> Arc<Mutex<HashSet<NonZeroU32>>> {
        self.conn_players.clone()
    }
    fn ensure_player_for_connection(&self, pid: NonZeroU32) -> Result<(), anyhow::Error> {
        let guard = self.conn_players.lock().unwrap();
        if guard.contains(&pid) { Ok(()) } else { bail!("player_id {} not registered for this connection", pid) }
    }
    fn parse_status(&self, v: &Value) -> Result<FsctStatus, anyhow::Error> {
        let code = v.as_u64().with_context(|| "invalid FsctStatus: expected integer")? as u8;
        let s = match code {
            0x00 => FsctStatus::Stopped,
            0x01 => FsctStatus::Playing,
            0x02 => FsctStatus::Paused,
            0x03 => FsctStatus::Seeking,
            0x04 => FsctStatus::Buffering,
            0x05 => FsctStatus::Error,
            0x0F => FsctStatus::Unknown,
            _ => bail!("invalid status code: {}", code),
        };
        Ok(s)
    }
    fn parse_timeline_opt(&self, v: &Value) -> Result<Option<TimelineInfo>, anyhow::Error> {
        if v.is_nil() { return Ok(None); }
        let m = v.as_map().with_context(|| "invalid TimelineInfo: expected map or nil")?;
        let mut position_ms: Option<u128> = None;
        let mut update_unix_ms: Option<i128> = None;
        let mut duration_ms: Option<u128> = None;
        let mut rate: Option<f64> = None;
        for (k, val) in m.iter() {
            if let Value::String(s) = k {
                if let Some(key) = s.as_str() {
                    match key {
                        "position_ms" => { position_ms = val.as_u64().map(|v| v as u128); }
                        "update_unix_ms" => { update_unix_ms = val.as_i64().map(|v| v as i128); }
                        "duration_ms" => { duration_ms = val.as_u64().map(|v| v as u128); }
                        "rate" => { rate = val.as_f64(); }
                        _ => bail!("invalid timeline key: {}", key),
                    }
                }
            }
        }
        let pos = position_ms.with_context(|| "timeline missing position_ms")?;
        let upd = update_unix_ms.with_context(|| "timeline missing update_unix_ms")?;
        let dur = duration_ms.with_context(|| "timeline missing duration_ms")?;
        let r = rate.with_context(|| "timeline missing rate")?;
        let position = Duration::from_millis(pos as u64);
        let duration = Duration::from_millis(dur as u64);
        let update_time = if upd >= 0 { UNIX_EPOCH + Duration::from_millis(upd as u64) } else { UNIX_EPOCH - Duration::from_millis((-upd) as u64) };
        Ok(Some(TimelineInfo { position, update_time, duration, rate: r }))
    }
    fn parse_text_metadata_id(&self, v: &Value) -> Result<FsctTextMetadata, anyhow::Error> {
        let code = v.as_u64().with_context(|| "invalid FsctTextMetadata: expected integer")? as u8;
        let m = match code {
            0x01 => FsctTextMetadata::CurrentTitle,
            0x02 => FsctTextMetadata::CurrentAuthor,
            0x03 => FsctTextMetadata::CurrentAlbum,
            0x04 => FsctTextMetadata::CurrentGenre,
            0x31 => FsctTextMetadata::QueueTitle,
            0x32 => FsctTextMetadata::QueueAuthor,
            0x33 => FsctTextMetadata::QueueAlbum,
            0x34 => FsctTextMetadata::QueueGenre,
            _ => bail!("invalid text metadata id: {}", code),
        };
        Ok(m)
    }
    fn parse_player_state_map(&self, v: &Value) -> Result<PlayerState, anyhow::Error> {
        let m = v.as_map().with_context(|| "invalid PlayerState: expected map")?;
        let mut status: Option<FsctStatus> = None;
        let mut timeline: Option<Option<TimelineInfo>> = None;
        let mut title: Option<Option<String>> = None;
        let mut artist: Option<Option<String>> = None;
        let mut album: Option<Option<String>> = None;
        let mut genre: Option<Option<String>> = None;
        let mut texts_found = false;
        for (k, val) in m.iter() {
            if let Value::String(s) = k {
                if let Some(key) = s.as_str() {
                    match key {
                        "status" => { status = Some(self.parse_status(val)?); }
                        "timeline" => { timeline = Some(self.parse_timeline_opt(val)?); }
                        "texts" => {
                            texts_found = true;
                            let tm = val.as_map().with_context(|| "texts must be map")?;
                            for (tk, tv) in tm.iter() {
                                if let Value::String(ts) = tk {
                                    if let Some(tkey) = ts.as_str() {
                                        match tkey {
                                            "title" => { if tv.is_nil() { title = Some(None); } else { title = Some(Some(tv.as_str().with_context(|| "text 'title' must be string or nil")?.to_string())); } }
                                            "artist" => { if tv.is_nil() { artist = Some(None); } else { artist = Some(Some(tv.as_str().with_context(|| "text 'artist' must be string or nil")?.to_string())); } }
                                            "album" => { if tv.is_nil() { album = Some(None); } else { album = Some(Some(tv.as_str().with_context(|| "text 'album' must be string or nil")?.to_string())); } }
                                            "genre" => { if tv.is_nil() { genre = Some(None); } else { genre = Some(Some(tv.as_str().with_context(|| "text 'genre' must be string or nil")?.to_string())); } }
                                            _ => bail!("invalid text key: {}", tkey),
                                        }
                                    }
                                }
                            }
                        }
                        _ => bail!("invalid player state key: {}", key),
                    }
                }
            }
        }
        let mut ps = PlayerState::default();
        ps.status = status.unwrap_or_default();
        ps.timeline = timeline.unwrap_or(None);
        let mut tm = TrackMetadata::default();
        if texts_found {
            if let Some(t) = title { tm.title = t; }
            if let Some(a) = artist { tm.artist = a; }
            if let Some(a2) = album { tm.album = a2; }
            if let Some(g) = genre { tm.genre = g; }
        }
        ps.texts = tm;
        Ok(ps)
    }
    fn handle_function<Params, Return, RequestAsyncOnDriver, ParseParamsFn, RequestFn>(
        &self,
        params: &[Value],
        parse_params: ParseParamsFn,
        request_async_on_driver: RequestFn,
    ) -> RequestFut
    where
        Params: Send + 'static,
        Return: Into<Value>,
        RequestAsyncOnDriver: Future<Output=Result<Return, anyhow::Error>> + Send + 'static,
        ParseParamsFn: FnOnce(&FsctRpcService, &[Value]) -> Result<Params, anyhow::Error>,
        RequestFn: FnOnce(Arc<dyn FsctDriver>, Params) -> RequestAsyncOnDriver + Send + 'static,
    {
        let parsed_params = parse_params(self, params);
        if let Err(e) = parsed_params {
            return fut_err(e.to_string());
        }
        let parsed_params = parsed_params.unwrap();
        let d = self.driver.clone();
        Box::pin(async move {
            let ret = request_async_on_driver(d, parsed_params)
                .await
                .map_err(|e| Value::from(e.to_string()))?;
            Ok(ret.into())
        })
    }
    fn req_get_protocol_version(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            |_, params|
                {
                    expect_params(params, 0, "")
                },
            async |_driver, _params|
                {
                    Ok(FSCT_PROTOCOL_VERSION.into_value())
                })
    }

    fn req_register_player(&self, params: &[Value]) -> RequestFut {
        // Custom to capture conn_players and insert on success
        let parse = (|| -> Result<String, anyhow::Error> {
            expect_params(params, 1, "self_id")?;
            params[0]
                .as_str()
                .map(String::from)
                .with_context(|| "invalid param: self_id must be string")
        })();
        if let Err(e) = parse { return fut_err(e.to_string()); }
        let self_id = parse.unwrap();
        let driver = self.driver.clone();
        let conn_players = self.conn_players.clone();
        let connection_id = self.connection_id.clone();
        Box::pin(async move {
            let pid = match driver
                .register_player(self_id)
                .await
                .with_context(|| "register_player error")
            {
                Ok(pid) => pid,
                Err(e) => return Err(Value::from(e.to_string())),
            };
            info!("Registered player {} for connection {}", pid.get(), connection_id);
            {
                let mut guard = conn_players.lock().unwrap();
                guard.insert(pid);
            }
            Ok(Value::from(pid.get() as u64))
        })
    }

    fn req_unregister_player(&self, params: &[Value]) -> RequestFut {
        let parse = (|| -> Result<NonZeroU32, anyhow::Error> {
            expect_params(params, 1, "player_id")?;
            let pid = parse_player_id(&params[0])?;
            self.ensure_player_for_connection(pid)?;
            Ok(pid)
        })();
        if let Err(e) = parse { return fut_err(e.to_string()); }
        let pid = parse.unwrap();
        let driver = self.driver.clone();
        let conn_players = self.conn_players.clone();
        let connection_id = self.connection_id.clone();
        Box::pin(async move {
            if let Err(e) = driver.unregister_player(pid).await.with_context(|| "unregister_player error") {
                return Err(Value::from(e.to_string()));
            }
            {
                let mut guard = conn_players.lock().unwrap();
                guard.remove(&pid);
            }
            info!("Unregistered player {} for connection {}", pid.get(), connection_id);
            Ok(Nil.into())
        })
    }

    fn parse_assign_player_to_device_params(&self, params: &[Value]) -> Result<(std::num::NonZeroU32, Uuid), anyhow::Error> {
        expect_params(params, 2, "player_id, device_id")?;
        let pid = parse_player_id(&params[0])?;
        self.ensure_player_for_connection(pid)?;
        let did = parse_device_id(&params[1])?;
        Ok((pid, did))
    }

    fn req_assign_player_to_device(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            Self::parse_assign_player_to_device_params,
            async |driver, (pid, did)| {
                driver
                    .assign_player_to_device(pid, did)
                    .await?;
                Ok(Nil)
            },
        )
    }

    fn req_unassign_player_from_device(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            Self::parse_assign_player_to_device_params,
            async |driver, (pid, did)| {
                driver
                    .unassign_player_from_device(pid, did)
                    .await
                    .with_context(|| "unassign_player_from_device error")?;
                Ok(Nil)
            },
        )
    }

    fn parse_update_player_state_params(&self, params: &[Value]) -> Result<(NonZeroU32, PlayerState), anyhow::Error> {
        expect_params(params, 2, "player_id, state")?;
        let pid = parse_player_id(&params[0])?;
        self.ensure_player_for_connection(pid)?;
        let state = self.parse_player_state_map(&params[1])?;
        Ok((pid, state))
    }

    fn req_update_player_state(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            Self::parse_update_player_state_params,
            async |driver, (pid, state)| {
                driver.update_player_state(pid, state).await?;
                Ok(Nil)
            },
        )
    }

    fn parse_update_player_status_params(&self, params: &[Value]) -> Result<(NonZeroU32, FsctStatus), anyhow::Error> {
        expect_params(params, 2, "player_id, status")?;
        let pid = parse_player_id(&params[0])?;
        self.ensure_player_for_connection(pid)?;
        let status = self.parse_status(&params[1])?;
        Ok((pid, status))
    }

    fn req_update_player_status(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            Self::parse_update_player_status_params,
            async |driver, (pid, status)| {
                driver.update_player_status(pid, status).await?;
                Ok(Nil)
            },
        )
    }

    fn parse_update_player_timeline_params(&self, params: &[Value]) -> Result<(NonZeroU32, Option<TimelineInfo>), anyhow::Error> {
        expect_params(params, 2, "player_id, timeline")?;
        let pid = parse_player_id(&params[0])?;
        self.ensure_player_for_connection(pid)?;
        let timeline = self.parse_timeline_opt(&params[1])?;
        Ok((pid, timeline))
    }

    fn req_update_player_timeline(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            Self::parse_update_player_timeline_params,
            async |driver, (pid, timeline)| {
                driver.update_player_timeline(pid, timeline).await?;
                Ok(Nil)
            },
        )
    }

    fn parse_update_player_metadata_params(&self, params: &[Value]) -> Result<(NonZeroU32, FsctTextMetadata, Option<String>), anyhow::Error> {
        expect_params(params, 3, "player_id, metadata_id, text")?;
        let pid = parse_player_id(&params[0])?;
        self.ensure_player_for_connection(pid)?;
        let meta = self.parse_text_metadata_id(&params[1])?;
        let text = if params[2].is_nil() { None } else { Some(params[2].as_str().with_context(|| "text must be string or nil")?.to_string()) };
        Ok((pid, meta, text))
    }

    fn req_update_player_metadata(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            Self::parse_update_player_metadata_params,
            async |driver, (pid, meta, text)| {
                driver.update_player_metadata(pid, meta, text).await?;
                Ok(Nil)
            },
        )
    }

    fn req_get_player_assigned_device(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            |s, params| {
                expect_params(params, 1, "player_id")?;
                let pid = parse_player_id(&params[0])?;
                s.ensure_player_for_connection(pid)?;
                Ok(pid)
            },
            async |driver, pid| {
                let opt = driver.get_player_assigned_device(pid).await?;
                let val = match opt {
                    Some(uuid) => Value::Binary(uuid.as_bytes().to_vec()),
                    None => Value::Nil
                };
                Ok(val)
            },
        )
    }
}

impl Service for FsctRpcService {
    type RequestFuture = Pin<Box<dyn Future<Output=Result<Value, Value>> + Send>>;

    fn handle_request(&mut self, method: &str, params: &[Value]) -> Self::RequestFuture {
        let m = method.to_string();
        match m.as_str() {
            "get_protocol_version" => self.req_get_protocol_version(params),
            "register_player" => self.req_register_player(params),
            "unregister_player" => self.req_unregister_player(params),
            "assign_player_to_device" => self.req_assign_player_to_device(params),
            "unassign_player_from_device" => self.req_unassign_player_from_device(params),
            "update_player_state" => self.req_update_player_state(params),
            "update_player_status" => self.req_update_player_status(params),
            "update_player_timeline" => self.req_update_player_timeline(params),
            "update_player_metadata" => self.req_update_player_metadata(params),
            "get_player_assigned_device" => self.req_get_player_assigned_device(params),
            _ => fut_err(format!("unknown method: {}", m)),
        }
    }

    fn handle_notification(&mut self, _method: &str, _params: &[Value]) {
        // No-op for now
    }
}

