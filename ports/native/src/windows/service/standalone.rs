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

use log::{info, error, debug, warn};
use tokio::runtime::Runtime;
use std::sync::Arc;
use anyhow::anyhow;
use fsct_core::{FsctDriver, LocalDriver, MultiServiceHandle};

use crate::cli::{Cli, LogLevel};
use crate::windows::service::logger::init_standalone_logger;
use tokio::signal::windows::ctrl_close;
use crate::run_os_watcher;
use crate::windows::socket_path;

async fn shutdown_signal() {
    debug!("Press Ctrl+C or close the console window to exit");

    // Create the ctrl_close handler
    let mut close_signal = ctrl_close().expect("Failed to create ctrl_close handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C signal, exiting...");
        }
        _ = close_signal.recv() => {
            info!("Received close signal from Windows, exiting...");
        }
    }
}

async fn standalone_task(args: Cli) -> anyhow::Result<()> {
    let endpoint = args.endpoint.clone().unwrap_or_else(|| socket_path().to_string());

    let mut services = MultiServiceHandle::new();

    let driver: Arc<dyn FsctDriver> = if args.user {
        // In user mode, connect to IPC driver
        info!("Connecting to IPC driver at {}", endpoint);
        Arc::new(fsct_core::ipc::client::IpcDriver::connect_to_endpoint(endpoint.clone()).await?)
    } else {
        // in driver and standalone mode use in-process driver
        info!("Starting in-process driver and orchestrator");
        let driver = Arc::new(LocalDriver::with_new_managers());
        services = driver.run().await.map_err(|e| anyhow!(e))?;
        driver
    };


    let ok  = if args.driver {
        info!("Starting IPC server at {}", endpoint);
        let ipc = fsct_core::ipc::server::run_ipc_server_with_endpoint_path(driver.clone(), endpoint.clone());
    services.add(ipc);
        true
    } else {
        // In user and standalone modes, start OS watcher and connect to driver (IPC or in-process)
        debug!("Starting GSMTC watcher (WindowsSystemPlayer)");
        if let Ok(watcher) = run_os_watcher(driver.clone()).await {
            services.add(watcher);
            true
        } else {
            warn!("Failed to start OS watcher");
            false
        }
    };

    if ok {
        shutdown_signal().await;
    }

    debug!("Shutting down services");

    let shutdown_res = services.shutdown().await
                               .inspect_err(|e| log::error!("Shutdown error: {}", e));

    shutdown_res?;
    Ok(())
}

// Function to run the service in standalone mode (for debugging)
pub fn run_standalone(log_level: LogLevel, args: Cli) -> anyhow::Result<()> {
    // todo support driver and user here
    
    // Initialize logger for standalone mode
    if let Err(e) = init_standalone_logger(log_level) {
        eprintln!("Failed to initialize logger: {}", e);
    }

    debug!("Starting in standalone mode with log level: {}", log_level);

    // Create a Tokio runtime for async operations
    debug!("Creating Tokio runtime");
    let rt = Runtime::new()?;

    // Run the service in the Tokio runtime
    rt.block_on(async {
        standalone_task(args).await
                         .map_err(|e| error!("Failed with error: {}", e))
                         .ok();
    });

    debug!("Standalone mode exited");
    Ok(())
}
