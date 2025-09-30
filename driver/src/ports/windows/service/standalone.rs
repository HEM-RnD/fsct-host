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

use log::{info, debug};
use tokio::runtime::Runtime;
use anyhow::anyhow;

use crate::cli::{LogLevel};
use crate::ports::windows::service::logger::init_standalone_logger;
use tokio::signal::windows::ctrl_close;
use crate::service_main::{async_main, StopSignal};
use crate::ServiceStateNullListener;

struct WindowsStandaloneStopSignal;

impl WindowsStandaloneStopSignal {
    fn new() -> Self { Self }
}

impl StopSignal for WindowsStandaloneStopSignal {
    async fn wait(&mut self) -> anyhow::Result<()> {
        debug!("Press Ctrl+C or close the console window to exit");

        // Create the ctrl_close handler
        let mut close_signal = ctrl_close().expect("Failed to create ctrl_close handler");

        tokio::select! {
            res = tokio::signal::ctrl_c() => {
                if res.is_ok() {
                    info!("Received Ctrl+C signal, exiting...");
                }
                res.map_err(|e| anyhow!(e))
            }
            res = close_signal.recv() => {
                if res.is_some() {
                    info!("Received close signal from Windows, exiting...");
                }
                res.ok_or_else(|| anyhow!("Error in receiving close signal"))
            }
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

    let stop_signal = WindowsStandaloneStopSignal::new();

    // Run the service in the Tokio runtime
    rt.block_on(async move {
        async_main(stop_signal, ServiceStateNullListener).await
    })
}
