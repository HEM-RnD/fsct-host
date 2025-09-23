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

use std::os::fd::{FromRawFd, OwnedFd};
use anyhow::anyhow;
use env_logger::Env;
use fsct_core::{FsctDriver, LocalDriver, MultiServiceHandle};
use std::sync::Arc;
use crate::run_os_watcher;

use crate::cli::Cli;
use clap::Parser;
use log::{info, warn};
use crate::socket_path;

/// Linux service entrypoint with CLI to choose mode (standalone/driver/user).
#[tokio::main(flavor = "current_thread")]
pub async fn fsct_main() -> anyhow::Result<()> {
    // Parse CLI
    let args = Cli::parse();

    // Initialize logging via env_logger (FSCT_LOG, FSCT_LOG_STYLE)
    let env = Env::default()
        .filter_or("FSCT_LOG", "info")
        .write_style("FSCT_LOG_STYLE");
    env_logger::init_from_env(env);
    let mut services = MultiServiceHandle::new();

    let endpoint = args.endpoint.clone().unwrap_or_else(|| socket_path().to_string());

    let driver: Arc<dyn FsctDriver> = if args.user {
        // In user mode, connect to IPC driver
        info!("Connecting to IPC driver at {}", endpoint);
        Arc::new(fsct_core::ipc::client::IpcDriver::connect_to_endpoint(endpoint.clone()).await?)
    } else {
        // in driver and standalone mode use in-process driver
        let driver = Arc::new(LocalDriver::with_new_managers());
        services = driver.run().await.map_err(|e| anyhow!(e))?;
        driver
    };

    let socket_activation_fd = crate::get_socket_activation_fd();

    let mut clean_socket = false;
    let mut ok = true;

    if args.driver {
        // In driver mode, expose IPC driver over IPC and do not start OS watcher
        let ipc = if let Some(fd) = socket_activation_fd {
            if args.endpoint.is_some() {
                warn!("Ignoring --socket argument because systemd socket activation is detected");
            }
            fsct_core::ipc::server::run_ipc_server_with_fd(driver.clone(), fd)
        } else {
            // remove potential stale socket, ignore if not present
            std::fs::remove_file(endpoint.as_str()).ok();
            // set socket to be removed in the end of the function
            clean_socket = true;
            fsct_core::ipc::server::run_ipc_server_with_endpoint_path(driver.clone(), endpoint.clone())
        };
        services.add(ipc);
    } else {
        // In user and standalone modes, start OS watcher and connect to driver (IPC or in-process)
        if let Ok(watcher) = run_os_watcher(driver.clone()).await {
            services.add(watcher);
        } else {
            warn!("Failed to start OS watcher");
            ok = false;
        }
    }

    // wait for finish only if everything started successfully, otherwise jump directly to shutdown
    let service_res = if ok {
        let mut terminate_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {Ok(())},
            _ = terminate_signal.recv() => {Ok(())},
            res = services.wait_for_any_to_finish() => {
                res.inspect_err(|e| log::error!("Service error: {}", e))
            },
        }
    } else {
        Ok(())
    };

    let shutdown_res = services.shutdown().await
        .inspect_err(|e| log::error!("Shutdown error: {}", e));

    if clean_socket == false {
        if let Err(r) = std::fs::remove_file(endpoint) {
            log::warn!("Failed to remove IPC socket file: {}", r);
        }
    }

    service_res?;
    shutdown_res?;
    Ok(())
}

