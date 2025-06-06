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
use fsct_core::run_service;
use crate::initialize_native_platform_player;
use crate::windows::service::cli::LogLevel;
use crate::windows::service::logger::init_standalone_logger;

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
        debug!("Initializing native platform player");
        let platform_global_player = match initialize_native_platform_player().await {
            Ok(player) => player,
            Err(e) => {
                error!("Failed to initialize player: {}", e);
                return;
            }
        };

        // Start the service
        debug!("Starting service");
        if let Err(e) = run_service(platform_global_player).await {
            error!("Service error: {}", e);
        }

        // Wait for Ctrl+C
        debug!("Press Ctrl+C to exit");
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C signal");
        info!("Received Ctrl+C signal, exiting...");
    });

    debug!("Standalone mode exited");
    Ok(())
}