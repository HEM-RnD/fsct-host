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
use fsct_core::{LocalDriver, MultiServiceHandle, FsctDriver};
use std::sync::Arc;
use crate::run_os_watcher;

mod cli;
use cli::Cli;
use clap::Parser;
use log::{info, warn};
use crate::linux::linux_local_socket_path;

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

    let endpoint = args.socket.clone().unwrap_or_else(|| linux_local_socket_path().to_string());

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

    let mut systemd_socket_activated = false;

    if args.driver {
        systemd_socket_activated = is_systemd_triggered_by_socket_activated();
        // In driver mode, expose IPC driver over IPC and do not start OS watcher

        let ipc = if systemd_socket_activated {
            info!("systemd socket activation detected, using fd 3");
            if args.socket.is_some() {
                warn!("Ignoring --socket argument because systemd socket activation is detected");
            }
            // If systemd socket activation is used, use the pre-opened listening socket (fd=3) passed by systemd/socket-activation helper.
            // 3 is the only fd that systemd/socket-activation helper will pass to the process,
            // and it has to be valid fd for the process to be able to use it.
            fsct_core::ipc::server::run_ipc_server_with_fd(driver.clone(), unsafe { OwnedFd::from_raw_fd(3) })
        } else {
            info!("systemd socket activation not detected, using {}", endpoint);
            // If not using systemd socket activation (LISTEN_FDS==0), remove a potential stale socket file.
            if let Err(e) = std::fs::remove_file(endpoint.as_str()) { let _ = e; /* ignore if not present */ }
            fsct_core::ipc::server::run_ipc_server_with_endpoint_path(driver.clone(), endpoint.clone())
        };
        services.add(ipc);
    } else {
        // In user and standalone modes, start OS watcher and connect to driver (IPC or in-process)
        if let Ok(watcher) = run_os_watcher(driver.clone()).await {
            services.add(watcher);
        }
    }

    let mut terminate_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let service_res = tokio::select! {
        _ = tokio::signal::ctrl_c() => {Ok(())},
        _ = terminate_signal.recv() => {Ok(())},
        res = services.wait_for_any_to_finish() => {
            res.inspect_err(|e| log::error!("Service error: {}", e))
        },
    };
    let shutdown_res = services.shutdown().await
        .inspect_err(|e| log::error!("Shutdown error: {}", e));

    if args.driver {
        // Only attempt to remove the socket file if we created it ourselves (no socket activation)
        if systemd_socket_activated == false {
            if let Err(r) = std::fs::remove_file(endpoint) {
                log::warn!("Failed to remove IPC socket file: {}", r);
            }
        }
    }

    service_res?;
    shutdown_res?;
    Ok(())
}

fn is_systemd_triggered_by_socket_activated() -> bool {
    let listen_fds = std::env::var("LISTEN_FDS").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let listen_pid = std::env::var("LISTEN_PID").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let this_process_pid = std::process::id();
    // Be sure that FDs are assigned to the correct (this) process; otherwise systemd will not pass them to us.
    listen_fds > 0 && listen_pid == this_process_pid
}
