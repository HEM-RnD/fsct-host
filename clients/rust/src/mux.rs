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

use std::collections::HashMap;

use fsct::DeviceChangeEvent;
use futures::{SinkExt, StreamExt};
use log::warn;
use serde_json::{Value, Value as JsonValue, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::sync::oneshot::Sender;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use uuid::Uuid;

use crate::rpc::{RpcNotification, RpcRequest, RpcResponse};

pub(crate) struct OutboundCall {
    pub(crate) method: String,
    pub(crate) params: JsonValue,
    pub(crate) reply: oneshot::Sender<Result<JsonValue, String>>,
}

type PendingMap = HashMap<u64, oneshot::Sender<Result<JsonValue, String>>>;

pub(crate) async fn run_mux<R, W>(
    mut reader: FramedRead<R, LinesCodec>,
    mut writer: FramedWrite<W, LinesCodec>,
    mut rx: mpsc::Receiver<OutboundCall>,
    device_tx: broadcast::Sender<DeviceChangeEvent>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut pending: PendingMap = HashMap::new();
    let mut next_id: u64 = 1;

    loop {
        tokio::select! {
            call = rx.recv() => {
                let Some(call) = call else { break };
                if handle_outbound(call, &mut writer, &mut pending, &mut next_id).await { break; }
            }
            line = reader.next() => {
                if handle_inbound(line, &mut pending, &device_tx) { break; }
            }
        }
    }
}

async fn handle_outbound<W: AsyncWrite + Unpin>(
    call: OutboundCall,
    writer: &mut FramedWrite<W, LinesCodec>,
    pending: &mut PendingMap,
    next_id: &mut u64,
) -> bool {
    let id = *next_id;
    *next_id += 1;
    let req = RpcRequest { jsonrpc: "2.0".into(), id: json!(id), method: call.method, params: call.params };
    match serde_json::to_string(&req) {
        Ok(line) => {
            if writer.send(line).await.is_err() {
                let _ = call.reply.send(Err("connection closed".into()));
                return true;
            }
            pending.insert(id, call.reply);
        }
        Err(e) => {
            let _ = call.reply.send(Err(format!("serialize error: {}", e)));
        }
    }
    false
}

fn handle_inbound(
    line_result: Option<Result<String, tokio_util::codec::LinesCodecError>>,
    pending: &mut PendingMap,
    device_tx: &broadcast::Sender<DeviceChangeEvent>,
) -> bool {
    let Some(line_result) = line_result else {
        fail_all_pending(pending, "connection closed");
        return true;
    };
    let line = match line_result {
        Ok(l) => l,
        Err(e) => {
            warn!("IPC read error: {}", e);
            fail_all_pending(pending, &format!("read error: {}", e));
            return true;
        }
    };
    dispatch_inbound(&line, pending, device_tx);
    false
}

fn fail_all_pending(pending: &mut PendingMap, reason: &str) {
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err(reason.into()));
    }
}

fn dispatch_inbound(
    line: &str,
    pending: &mut HashMap<u64, oneshot::Sender<Result<JsonValue, String>>>,
    device_tx: &broadcast::Sender<DeviceChangeEvent>,
) {
    if let Ok(resp) = serde_json::from_str::<RpcResponse>(line) {
        handle_response(pending, resp);
    } else if let Ok(notif) = serde_json::from_str::<RpcNotification>(line) {
        handle_notification(&notif, device_tx);
    } else {
        warn!("IPC: unrecognized message: {}", &line[..line.len().min(200)]);
    }
}

fn handle_response(pending: &mut HashMap<u64, Sender<Result<Value, String>>>, resp: RpcResponse) {
    if let Some(id_u64) = resp.id.as_u64() {
        if let Some(tx) = pending.remove(&id_u64) {
            let result = resp.error.map(|e| Err(e.message)).unwrap_or_else(|| Ok(resp.result.unwrap_or(JsonValue::Null)));
            let _ = tx.send(result);
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
    let uuid = match Uuid::parse_str(device_id_str) {
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
