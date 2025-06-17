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

// Re-export modules
pub mod cli;
pub mod constants;
pub mod install;
pub mod logger;
pub mod runtime;
pub mod standalone;

// Re-export commonly used items
pub use cli::{Cli, Commands, ServiceCommands, LogLevel};
pub use constants::{SERVICE_NAME, SERVICE_DISPLAY_NAME, SERVICE_DESCRIPTION};
pub use install::{install_service, uninstall_service};
pub use logger::{init_service_logger, init_install_logger, init_standalone_logger};
pub use runtime::service_main;
pub use standalone::run_standalone;

use anyhow::bail;
use log::{info, error, debug};
use clap::Parser;

pub fn fsct_main() -> anyhow::Result<()> {
    // Parse command line arguments using clap
    let cli = Cli::parse();
    let log_level = cli.log_level;

    // Check if a command was provided
    if let Some(command) = cli.command {
        match command {
            Commands::Service { command } => {
                match command {
                    ServiceCommands::Install { verbose, service_log_level,  user_service} => {
                        // Initialize logger for install command
                        if let Err(e) = init_install_logger(verbose, log_level) {
                            eprintln!("Failed to initialize logger: {}", e);
                            bail!("Failed to initialize logger: {}", e);
                        }
                        debug!("Installing service with log level: {}", log_level);
                        let result = install_service(service_log_level, user_service);
                        if let Err(ref e) = result {
                            error!("Failed to install service: {}", e);
                        } else {
                            info!("Service installed successfully");
                        }
                        return result;
                    }
                    ServiceCommands::Uninstall { verbose } => {
                        // Initialize logger for uninstall command
                        if let Err(e) = init_install_logger(verbose, log_level) {
                            eprintln!("Failed to initialize logger: {}", e);
                            bail!("Failed to initialize logger: {}", e);
                        }
                        debug!("Uninstalling service with log level: {}", log_level);
                        let result = uninstall_service();
                        if let Err(ref e) = result {
                            error!("Failed to uninstall service: {}", e);
                        } else {
                            info!("Service uninstalled successfully");
                        }
                        return result;
                    }
                    ServiceCommands::Run => {
                        // Initialize the logger first thing
                        if let Err(e) = init_service_logger(log_level) {
                            // Can't log this error since the logger failed to initialize
                            eprintln!("Failed to initialize logger: {}", e);
                            bail!("Failed to initialize logger: {}", e);
                        }
                        // Run as a service
                        info!("Service starting with log level: {}", log_level);
                        return runtime::start_service();
                    }
                }
            }
        }
    }

    // If no arguments provided, run in standalone mode
    run_standalone(log_level)
}