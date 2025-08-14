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

use log::{info, error, debug};
use tokio::runtime::Runtime;
use std::sync::Arc;
use fsct_core::LocalDriver;

use crate::windows::player::WindowsSystemPlayer;
use crate::windows::service::cli::LogLevel;
use crate::windows::service::logger::init_standalone_logger;
use tokio::signal::windows::ctrl_close;

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

// Function to run the service in standalone mode (for debugging)
pub fn run_standalone(log_level: LogLevel) -> anyhow::Result<()> {
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
        debug!("Creating LocalDriver and starting services");
        let driver = Arc::new(LocalDriver::with_new_managers());

        debug!("Starting orchestrator + USB watch via LocalDriver::run()");
        let services = match driver.run().await {
            Ok(handle) => {
                debug!("Services started successfully");
                Some(handle)
            }
            Err(e) => {
                error!("Failed to start services: {}", e);
                None
            }
        };

        debug!("Starting GSMTC watcher (WindowsSystemPlayer)");
        let _watcher = match WindowsSystemPlayer::new_with_driver(driver.clone()).await {
            Ok(w) => Some(w),
            Err(e) => {
                error!("Failed to start GSMTC watcher: {:?}", e);
                None
            }
        };

        // Wait for Ctrl+C or shutdown signal
        shutdown_signal().await;

        // Shutdown services if they were started successfully
        if let Some(handle) = services {
            debug!("Shutting down services");
            if let Err(e) = handle.shutdown().await {
                error!("Error shutting down services: {}", e);
            }
        }
    });

    debug!("Standalone mode exited");
    Ok(())
}
