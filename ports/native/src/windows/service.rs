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

use std::ffi::OsString;
use windows_service::{
    service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
    },
    service_manager::{ServiceManager, ServiceManagerAccess},
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    define_windows_service,
};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use anyhow::Result;
use log::{info, error, LevelFilter};
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
};
use tokio::runtime::Runtime;
use fsct_core::run_service;
use crate::initialize_native_platform_player;

pub const SERVICE_NAME: &str = "FsctDriverService";
pub const SERVICE_DISPLAY_NAME: &str = "FSCT Driver Service";
pub const SERVICE_DESCRIPTION: &str = "Ferrum Streaming Control Technology Driver Service";

fn get_service_type() -> ServiceType
{ ServiceType::USER_OWN_PROCESS | ServiceType::INTERACTIVE_PROCESS }

pub fn install_service() -> Result<()> {
    info!("Starting service installation");

    info!("Connecting to service manager");
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = match ServiceManager::local_computer(None::<&str>, manager_access) {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to connect to service manager: {}", e);
            return Err(e.into());
        }
    };

    // Get the current executable path
    info!("Getting current executable path");
    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            error!("Failed to get current executable path: {}", e);
            return Err(e.into());
        }
    };

    let service_binary_path = match current_exe.to_str() {
        Some(path) => path,
        None => {
            let err = anyhow::anyhow!("Invalid path");
            error!("Invalid executable path: {}", err);
            return Err(err);
        }
    };

    info!("Service binary path: {}", service_binary_path);

    // Create the service info
    info!("Creating service info");
    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: get_service_type(),
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: PathBuf::from(service_binary_path),
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None, // Run as LocalSystem
        account_password: None,
    };

    // Create the service
    info!("Creating service");
    let service = match service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG) {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to create service: {}", e);
            return Err(e.into());
        }
    };

    // Set the service description
    info!("Setting service description");
    if let Err(e) = service.set_description(SERVICE_DESCRIPTION) {
        error!("Failed to set service description: {}", e);
        return Err(e.into());
    }

    info!("Service installed successfully");
    println!("Service installed successfully");
    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    info!("Starting service uninstallation");

    info!("Connecting to service manager");
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = match ServiceManager::local_computer(None::<&str>, manager_access) {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to connect to service manager: {}", e);
            return Err(e.into());
        }
    };

    info!("Opening service: {}", SERVICE_NAME);
    let service_access = ServiceAccess::DELETE;
    let service = match service_manager.open_service(SERVICE_NAME, service_access) {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to open service: {}", e);
            return Err(e.into());
        }
    };

    // Delete the service
    info!("Deleting service");
    if let Err(e) = service.delete() {
        error!("Failed to delete service: {}", e);
        return Err(e.into());
    }
    info!("Service uninstalled successfully");
    println!("Service uninstalled successfully");
    Ok(())
}

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

pub fn fsct_main() -> anyhow::Result<()> {
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

fn run_service_main(_arguments: Vec<OsString>) -> anyhow::Result<()> {
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
        service_type: get_service_type(),
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
        service_type: get_service_type(),
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
