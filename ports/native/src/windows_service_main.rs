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

use fsct_core::run_service;
use fsct_native_port::initialize_native_platform_player;
use fsct_native_port::windows::service::{SERVICE_NAME, install_service, uninstall_service};
use log::{error, info, LevelFilter};
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tokio::runtime::Runtime;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

define_windows_service!(ffi_service_main, service_main);

fn init_logger() -> anyhow::Result<()> {
    // Create a log directory in ProgramData
    let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    let log_dir = PathBuf::from(program_data).join("FSCT");

    // Create the directory if it doesn't exist
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)?;
    }

    let log_file = log_dir.join("fsct_service.log");

    // Create a file appender
    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}\n")))
        .build(log_file)?;

    // Build the logger configuration
    let config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .build(Root::builder().appender("file").build(LevelFilter::Info))?;

    // Initialize the logger
    log4rs::init_config(config)?;

    info!("Logger initialized");
    Ok(())
}

// Function to run the service in standalone mode (for debugging)
fn run_standalone() -> anyhow::Result<()> {
    // Initialize console logger for standalone mode
    env_logger::Builder::from_env(env_logger::Env::default()
        .filter_or("FSCT_LOG", "info")
        .write_style("FSCT_LOG_STYLE"))
        .init();

    info!("Starting in standalone mode");

    // Create a Tokio runtime for async operations
    info!("Creating Tokio runtime");
    let rt = Runtime::new()?;

    // Run the service in the Tokio runtime
    rt.block_on(async {
        info!("Initializing native platform player");
        let platform_global_player = match initialize_native_platform_player().await {
            Ok(player) => player,
            Err(e) => {
                error!("Failed to initialize player: {}", e);
                return;
            }
        };

        // Start the service
        info!("Starting service");
        if let Err(e) = run_service(platform_global_player).await {
            error!("Service error: {}", e);
        }

        // Wait for Ctrl+C
        info!("Press Ctrl+C to exit");
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C signal");
        info!("Received Ctrl+C signal, exiting...");
    });

    info!("Standalone mode exited");
    Ok(())
}

pub(crate) fn fsct_main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "install" => {
                // Initialize logger for install command
                if let Err(e) = init_logger() {
                    eprintln!("Failed to initialize logger: {}", e);
                }
                info!("Installing service");
                let result = install_service();
                if let Err(ref e) = result {
                    error!("Failed to install service: {}", e);
                } else {
                    info!("Service installed successfully");
                }
                return result;
            }
            "uninstall" => {
                // Initialize logger for uninstall command
                if let Err(e) = init_logger() {
                    eprintln!("Failed to initialize logger: {}", e);
                }
                info!("Uninstalling service");
                let result = uninstall_service();
                if let Err(ref e) = result {
                    error!("Failed to uninstall service: {}", e);
                } else {
                    info!("Service uninstalled successfully");
                }
                return result;
            }
            "--standalone" => {
                // Run in standalone mode (for debugging)
                return run_standalone();
            }
            _ => {
                println!("Unknown command: {}", args[1]);
                println!("Available commands: install, uninstall, --standalone");
                return Ok(());
            }
        }
    }

    // If no arguments provided, run as a service
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

fn service_main(arguments: Vec<OsString>) {
    // Initialize the logger first thing
    if let Err(e) = init_logger() {
        // Can't log this error since the logger failed to initialize
        eprintln!("Failed to initialize logger: {}", e);
        return;
    }

    info!("Service starting");

    if let Err(e) = run_service_main(arguments) {
        error!("Service failed: {}", e);
    }
}

fn run_service_main(arguments: Vec<OsString>) -> anyhow::Result<()> {
    // Create a channel to communicate with the service control handler
    let (shutdown_tx, shutdown_rx) = mpsc::channel();

    // Register the service control handler
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                // Send shutdown signal
                info!("Received stop control event");
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => {
                info!("Received unsupported control event");
                ServiceControlHandlerResult::NotImplemented
            }
        }
    };

    info!("Registering service control handler");
    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    // Tell the system that the service is running
    info!("Setting service status to Running");
    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // Create a Tokio runtime for async operations
    info!("Creating Tokio runtime");
    let rt = Runtime::new()?;

    // Run the service in the Tokio runtime
    rt.block_on(async {
        info!("Initializing native platform player");
        let platform_global_player = match initialize_native_platform_player().await {
            Ok(player) => player,
            Err(e) => {
                error!("Failed to initialize player: {}", e);
                return;
            }
        };

        // Start the service
        info!("Starting service");
        if let Err(e) = run_service(platform_global_player).await {
            error!("Service error: {}", e);
        }

        // Create a future that completes when a shutdown signal is received
        let shutdown_future = async {
            let _ = shutdown_rx.recv();
        };

        // Wait for the shutdown signal
        info!("Waiting for shutdown signal");
        tokio::select! {
            _ = shutdown_future => {
                info!("Received shutdown signal");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C signal");
            }
        }

        info!("Exiting service");
    });

    // Tell the system that the service has stopped
    info!("Setting service status to Stopped");
    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    info!("Service stopped successfully");
    Ok(())
}
