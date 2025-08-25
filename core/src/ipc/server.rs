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

use crate::FsctDriver;

use msgpack_rpc::{serve, Service, Value};
use std::future::Future;
use std::pin::Pin;

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

        let mut incoming = Endpoint::new(endpoint.clone()).incoming().map_err(|e| anyhow::anyhow!("Failed to start IPC endpoint: {e}"))?;

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

async fn handle_connection<S>(stream: S, driver: Arc<dyn FsctDriver>) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    debug!("New IPC client connected");

    #[derive(Clone)]
    struct FsctRpcService {
        driver: Arc<dyn FsctDriver>,
    }

    impl Service for FsctRpcService {
        type RequestFuture = Pin<Box<dyn Future<Output = Result<Value, Value>> + Send>>;

        fn handle_request(&mut self, method: &str, params: &[Value]) -> Self::RequestFuture {
            let d = self.driver.clone();
            let m = method.to_string();
            let param_len = params.len();
            Box::pin(async move {
                match m.as_str() {
                    "get_protocol_version" => {
                        if param_len != 0 {
                            return Err("params not expected".into());
                        }
                        let v = d.get_protocol_version();
                        let result = Value::Map(vec![
                            (Value::from("major"), Value::from(v.major as u64)),
                            (Value::from("minor"), Value::from(v.minor as u64)),
                        ]);
                        Ok(result)
                    }
                    _ => Err(format!("unknown method: {}", m).into()),
                }
            })
        }

        fn handle_notification(&mut self, _method: &str, _params: &[Value]) {
            // No-op for now
        }
    }

    let service = FsctRpcService { driver };
    let mut compat_stream = stream.compat();
    serve(&mut compat_stream, service)
        .await
        .map_err(|e| anyhow::anyhow!("msgpack-rpc serve error: {}", e))
}
