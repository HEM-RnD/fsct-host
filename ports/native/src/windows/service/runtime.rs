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

use std::ffi::{OsStr, OsString};
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use clap::Parser;
use log::{info, error, debug};
use tokio::select;
use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use windows_service::{
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceAccess,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
    define_windows_service,
};
use windows_service::service::ServiceType;
use fsct_core::{FsctDriver, LocalDriver, MultiServiceHandle};
use fsct_core::ipc::client::IpcDriver;
use crate::run_os_watcher;
use crate::cli::Cli;
use crate::windows::service::get_service_name;
use crate::windows::socket_path;

// Define service events
#[derive(Clone)]
pub enum ServiceEvent {
    Shutdown,
}

pub fn get_current_session_id() -> Option<u32> {
    // Get the current active session ID using the Windows API
    let session_id = unsafe { WTSGetActiveConsoleSessionId() };
    if session_id == 0xFFFFFFFF {
        return None;
    }
    Some(session_id)
}

define_windows_service!(ffi_service_main, service_main);

// Public function to start the service
pub fn start_service(user_service: bool) -> Result<()> {
    let service_name = get_service_name(user_service);
    service_dispatcher::start(service_name, ffi_service_main)?;
    Ok(())
}

pub fn service_main(arguments: Vec<OsString>) {
    if let Err(e) = run_service_main(arguments) {
        error!("Service failed: {}", e);
    }
}

fn get_service_type_from_manager(name: impl AsRef<OsStr>) -> anyhow::Result<ServiceType> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(name, ServiceAccess::QUERY_CONFIG)?;
    let config = service.query_config()?;
    Ok(config.service_type)
}

fn get_socket_name(cli: &Cli) -> String {
    if let Some(endpoint) = &cli.endpoint {
        endpoint.clone()
    } else {
        socket_path().to_string()
    }
}

pub fn run_service_main(arguments: Vec<OsString>) -> anyhow::Result<()> {
    let cli = Cli::parse();
    debug!("Parsed arguments: {:?}", cli);

    let service_name = get_service_name(cli.user);

    debug!("Creating Tokio runtime");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    // Create a broadcast channel for events that can be used from both sync and async contexts
    let (event_tx, _) = tokio::sync::broadcast::channel::<ServiceEvent>(2);

    // Clone the sender for use in the service control handler
    let event_tx_clone = event_tx.clone();

    // Register the service control handler
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                // Send shutdown event
                debug!("Received stop control event");
                let _ = event_tx_clone.send(ServiceEvent::Shutdown);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => {
                debug!("Received unsupported control event: {:?}", control_event);
                ServiceControlHandlerResult::NotImplemented
            }
        }
    };
    let service_type = get_service_type_from_manager(service_name)?;

    debug!("Registering service control handler");
    let status_handle = service_control_handler::register(service_name, event_handler)?;

    // Tell the system that the service is starting
    debug!("Setting service status to StartPending");
    status_handle.set_service_status(ServiceStatus {
        service_type,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let endpoint_name = get_socket_name(&cli);

    // Run the service in the Tokio runtime
    rt.block_on(async {

        // Run driver
        debug!("Initializing driver");

        let (driver, mut service_handle) = if cli.driver {
            let driver = Arc::new(LocalDriver::with_new_managers());
            let mut service_handle = match driver.clone().run().await
            {
                Ok(driver_handle) => driver_handle,
                Err(e) => {
                    error!("Failed to run driver: {}", e);
                    return;
                }
            };
            let ipc_server_handle = fsct_core::ipc::server::run_ipc_server_with_endpoint_path(
                driver.clone(),
                endpoint_name.clone());
            service_handle.add(ipc_server_handle);
            (driver as Arc<dyn FsctDriver>, service_handle)
        } else {
            let ipc_driver = match IpcDriver::connect_to_endpoint(endpoint_name.clone()).await {
                Ok(driver) => driver,
                Err(e) => {
                    error!("Failed init IPC driver: {}", e);
                    return;
                }
            };
            (Arc::new(ipc_driver) as Arc<dyn FsctDriver>, MultiServiceHandle::new())
        };

        // Initialize the player
        let success = if cli.user {
            debug!("Initializing native platform player");
            let mut retries = 0;
            loop {
                match run_os_watcher(driver.clone()).await {
                    Ok(player) => {
                        service_handle.add(player);
                        break true
                    },
                    Err(e) => {
                        retries += 1;
                        if retries >= 10 {
                            error!("Failed to initialize player after 10 retries: {:?}", e);
                            break false
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        debug!("Retrying initialization, attempt {}/10", retries + 1);
                    }
                }
            }
        } else {
            true
        };

        // Tell the system that the service is running
        debug!("Setting service status to Running");
        let result = status_handle.set_service_status(ServiceStatus {
            service_type,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
        if let Err(e) = result {
            error!("Failed to set service status: {}", e);
            return;
        }

        // Create a receiver for the broadcast channel
        let mut event_rx = event_tx.subscribe();

        // Also listen for Ctrl+C
        let event_tx_ctrl_c = event_tx.clone();
        tokio::spawn(async move {
            if let Ok(_) = tokio::signal::ctrl_c().await {
                debug!("Received Ctrl+C signal");
                let _ = event_tx_ctrl_c.send(ServiceEvent::Shutdown);
            }
        });

        // Wait for events if initialized successfully
        if success {
            select! {
                res = event_rx.recv() => {
                    res
                    .inspect_err(|e| log::error!("Failed to receive event: {}", e))
                    .inspect(|_| info!("Received shutdown event, stopping...")).ok();
                },
                res = service_handle.wait_for_any_to_finish() => {
                    res.inspect_err(|e| log::error!("Service error: {}", e)).ok();
                }
            }
        };

        // Tell the system that the service has stopped
        debug!("Setting service status to Stopped");
        status_handle.set_service_status(ServiceStatus {
            service_type,
            current_state: ServiceState::StopPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        }).ok();

        // Stop the service tasks
        debug!("Stopping service tasks");
        if let Err(e) = service_handle.shutdown().await
        {
            error!("Failed to stop service tasks: {}", e);
        }

        info!("Exiting service");
    });

    rt.shutdown_timeout(Duration::from_secs(10));
    debug!("Service tasks stopped, exiting");

    // Tell the system that the service has stopped
    debug!("Setting service status to Stopped");
    status_handle.set_service_status(ServiceStatus {
        service_type,
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
