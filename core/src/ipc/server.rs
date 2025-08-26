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

//! IPC server (phase 2) using parity-tokio-ipc for transport and MessagePack(-RPC style) framing.
//!
//! The server currently implements a minimal subset required by docs/ipc_plan.md phase 2:
//! - Accept connections on a local endpoint
//! - Handle msgpack-rpc style requests for `get_protocol_version`
//! - Forward to the provided FsctDriver

use std::sync::Arc;

use log::{debug, error, info, warn};
use parity_tokio_ipc::Endpoint;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio::task::JoinSet;
use futures::StreamExt;

use crate::{FsctDriver, ProtocolVersion};
use crate::player_state::{PlayerState, TrackMetadata};
use crate::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use std::time::{Duration, UNIX_EPOCH};
use uuid::Uuid;
use std::num::NonZeroU32;
use crate::FSCT_PROTOCOL_VERSION;

use msgpack_rpc::{serve, Service, Value};
use std::future::Future;
use std::pin::Pin;
use anyhow::{anyhow, bail, Context};

/// Default endpoint resolver based on platform and optional FSCT_IPC_ENDPOINT override.
fn default_endpoint() -> String {
    if let Ok(override_ep) = std::env::var("FSCT_IPC_ENDPOINT") {
        if !override_ep.trim().is_empty() {
            return override_ep;
        }
    }
    // Windows Named Pipe path or Unix Domain Socket path
    #[cfg(windows)]
    { r"\\.\pipe\fsct_host_v1".to_string() }
    #[cfg(unix)]
    {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        format!("{base}/fsct/fsct.sock")
    }
}

/// IPC server that exposes FsctDriver API over a local IPC connection.
pub struct IpcServer {
    endpoint: String,
    driver: Arc<dyn FsctDriver>,
}

impl IpcServer {
    /// Create a new IpcServer bound to the given driver. Endpoint is taken from FSCT_IPC_ENDPOINT or platform default.
    pub fn new(driver: Arc<dyn FsctDriver>) -> Self {
        Self { endpoint: default_endpoint(), driver }
    }

    /// Create with an explicit endpoint path (useful for tests).
    pub fn with_endpoint(driver: Arc<dyn FsctDriver>, endpoint: String) -> Self {
        Self { endpoint, driver }
    }

    /// Start serving and block until the accept loop terminates (e.g., due to unrecoverable error or shutdown signal via drop).
    pub async fn serve(&self) -> anyhow::Result<()> {
        let endpoint = &self.endpoint;
        info!("FSCT IPC server listening on: {}", endpoint);

        // For unix, ensure directory exists with correct perms. Keep minimal for now per phase 2.
        #[cfg(unix)]
        {
            if let Some(parent) = std::path::Path::new(endpoint).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // Remove stale socket if any
            let _ = std::fs::remove_file(endpoint);
        }

        let incoming = Endpoint::new(endpoint.clone()).incoming().map_err(|e| anyhow::anyhow!("Failed to start IPC endpoint: {e}"))?;

        let mut tasks = JoinSet::new();
        let driver = self.driver.clone();

        tokio::pin!(incoming);
        loop {
            match incoming.as_mut().next().await {
                Some(Ok(stream)) => {
                    let driver = driver.clone();
                    tasks.spawn(async move {
                        if let Err(e) = handle_connection(stream, driver).await {
                            warn!("IPC connection handler ended with error: {e:?}");
                        }
                    });
                }
                Some(Err(e)) => {
                    error!("IPC accept failed: {}", e);
                    break;
                }
                None => {
                    // incoming stream ended
                    break;
                }
            }

            // Reap finished tasks to avoid memory growth
            while let Some(res) = tasks.try_join_next() {
                if let Err(e) = res { warn!("IPC connection task panicked: {e:?}"); }
            }
        }

        Ok(())
    }
}

#[derive(Clone)]
struct FsctRpcService {
    driver: Arc<dyn FsctDriver>,
}

// Request future type alias used by per-method handlers
type RequestFut = Pin<Box<dyn Future<Output=Result<Value, Value>> + Send>>;

// Small helper to produce an immediate error future without an async block
fn fut_err<V: Into<Value>>(v: V) -> RequestFut {
    Box::pin(std::future::ready(Err(v.into())))
}

impl Into<Value> for ProtocolVersion {
    fn into(self) -> Value {
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
            if let Value::String(s) = k { if let Some(key) = s.as_str() {
                match key {
                    "position_ms" => { position_ms = val.as_u64().map(|v| v as u128); },
                    "update_unix_ms" => { update_unix_ms = val.as_i64().map(|v| v as i128); },
                    "duration_ms" => { duration_ms = val.as_u64().map(|v| v as u128); },
                    "rate" => { rate = val.as_f64(); },
                    _ => bail!("invalid timeline key: {}", key),
                }
            }}
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
            if let Value::String(s) = k { if let Some(key) = s.as_str() { match key {
                "status" => { status = Some(self.parse_status(val)?); },
                "timeline" => { timeline = Some(self.parse_timeline_opt(val)?); },
                "texts" => {
                    texts_found = true;
                    let tm = val.as_map().with_context(|| "texts must be map")?;
                    for (tk, tv) in tm.iter() {
                        if let Value::String(ts) = tk { if let Some(tkey) = ts.as_str() { match tkey {
                            "title" => { if tv.is_nil() { title = Some(None); } else { title = Some(Some(tv.as_str().with_context(|| "text 'title' must be string or nil")?.to_string())); } },
                            "artist" => { if tv.is_nil() { artist = Some(None); } else { artist = Some(Some(tv.as_str().with_context(|| "text 'artist' must be string or nil")?.to_string())); } },
                            "album" => { if tv.is_nil() { album = Some(None); } else { album = Some(Some(tv.as_str().with_context(|| "text 'album' must be string or nil")?.to_string())); } },
                            "genre" => { if tv.is_nil() { genre = Some(None); } else { genre = Some(Some(tv.as_str().with_context(|| "text 'genre' must be string or nil")?.to_string())); } },
                            _ => bail!("invalid text key: {}", tkey),
                        }}}
                    }
                },
                _ => bail!("invalid player state key: {}", key),
            }}}
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
                    Ok(FSCT_PROTOCOL_VERSION)
                })
    }

    fn req_register_player(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            |_, params|
                {
                    expect_params(params, 1, "self_id")?;
                    params[0]
                        .as_str()
                        .map(String::from)
                        .with_context(|| "invalid param: self_id must be string")
                },
            async |driver, self_id| {
                driver
                    .register_player(self_id)
                    .await
                    .map(|v| v.get())
                    .with_context(|| "register_player error")
            })
    }

    fn req_unregister_player(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            |_, params|
                {
                    expect_params(params, 1, "player_id")?;
                    parse_player_id(&params[0])
                },
            async |driver, pid| {
                driver
                    .unregister_player(pid)
                    .await
                    .with_context(|| "unregister_player error")?;
                Ok(Nil)
            })
    }

    fn parse_assign_player_to_device_params(&self, params: &[Value]) -> Result<(std::num::NonZeroU32, Uuid), anyhow::Error> {
        expect_params(params, 2, "player_id, device_id")?;
        let pid = parse_player_id(&params[0])?;
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

    fn parse_optional_player_id(&self, v: &Value) -> Result<Option<NonZeroU32>, anyhow::Error> {
        if v.is_nil() { return Ok(None); }
        let pid = parse_player_id(v)?;
        Ok(Some(pid))
    }

    fn req_set_preferred_player(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            |s, params| {
                expect_params(params, 1, "preferred_player_id")?;
                s.parse_optional_player_id(&params[0])
            },
            async |driver, preferred| {
                driver.set_preferred_player(preferred).await?;
                Ok(Nil)
            },
        )
    }

    fn req_get_preferred_player(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            |_, params| {
                expect_params(params, 0, "")
            },
            async |driver, _| {
                let pref = driver.get_preferred_player().await;
                let val = match pref { Some(pid) => Value::from(pid.get() as u64), None => Value::Nil };
                Ok(val)
            },
        )
    }

    fn req_get_player_assigned_device(&self, params: &[Value]) -> RequestFut {
        self.handle_function(
            params,
            |_, params| {
                expect_params(params, 1, "player_id")?;
                let pid = parse_player_id(&params[0])?;
                Ok(pid)
            },
            async |driver, pid| {
                let opt = driver.get_player_assigned_device(pid).await?;
                let val = match opt { Some(uuid) => Value::Binary(uuid.as_bytes().to_vec()), None => Value::Nil };
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
            "set_preferred_player" => self.req_set_preferred_player(params),
            "get_preferred_player" => self.req_get_preferred_player(params),
            "get_player_assigned_device" => self.req_get_player_assigned_device(params),
            _ => fut_err(format!("unknown method: {}", m)),
        }
    }

    fn handle_notification(&mut self, _method: &str, _params: &[Value]) {
        // No-op for now
    }
}


async fn handle_connection<S>(stream: S, driver: Arc<dyn FsctDriver>) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    debug!("New IPC client connected");


    let service = FsctRpcService { driver };
    let mut compat_stream = stream.compat();
    serve(&mut compat_stream, service)
        .await
        .map_err(|e| anyhow::anyhow!("msgpack-rpc serve error: {}", e))
}
