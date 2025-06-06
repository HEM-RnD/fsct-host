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
use std::time::Duration;
use anyhow::Result;
use log::{info, error, debug};
use tokio::runtime::Runtime;
use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
use windows_service::{
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    define_windows_service,
};
use crate::windows::service::constants::SERVICE_NAME;
use crate::windows::service::install::get_service_type;
use crate::windows::service::state::FsctServiceState;

// Define service events
#[derive(Clone)]
pub enum ServiceEvent {
    Shutdown,
    SessionChange(windows_service::service::SessionChangeParam),
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
pub fn start_service() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

pub fn service_main(arguments: Vec<OsString>) {
    if let Err(e) = run_service_main(arguments) {
        error!("Service failed: {}", e);
    }
}

pub fn run_service_main(_arguments: Vec<OsString>) -> anyhow::Result<()> {
    // Create a Tokio runtime for async operations
    debug!("Creating Tokio runtime");
    let rt = Runtime::new()?;

    // Create a broadcast channel for events that can be used from both sync and async contexts
    let (event_tx, _) = tokio::sync::broadcast::channel::<ServiceEvent>(10);

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
            ServiceControl::SessionChange(param) => {
                debug!("Received session change event: {:?}, session ID: {}", param.reason, param.notification.session_id);
                let _ = event_tx_clone.send(ServiceEvent::SessionChange(param));
                ServiceControlHandlerResult::NoError
            }
            _ => {
                debug!("Received unsupported control event: {:?}", control_event);
                ServiceControlHandlerResult::NotImplemented
            }
        }
    };

    debug!("Registering service control handler");
    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    // Tell the system that the service is running
    debug!("Setting service status to Running");
    status_handle.set_service_status(ServiceStatus {
        service_type: get_service_type(),
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SESSION_CHANGE,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // Run the service in the Tokio runtime
    rt.block_on(async {
        // Create a service state to manage the service tasks
        let mut service_state = match FsctServiceState::new() {
            Ok(state) => state,
            Err(e) => {
                error!("Failed to create service state: {}", e);
                return;
            }
        };

        // Get the current active console session ID
        // This is the session ID of the user who is currently logged on to the physical console
        let current_session_id = get_current_session_id();
        service_state.assigned_session_id = current_session_id;  // Store the assigned session ID
        info!("Assigned session ID: {:?}", current_session_id);

        // Note: The assigned session ID is the session ID of the user who is currently logged on to the physical console
        // when the service starts. This is the session that the service is assigned to and should run for.
        // We only start service tasks for this session and stop them for all other sessions.

        // Start the service tasks
        if let Err(e) = service_state.start_service().await {
            error!("Failed to start service tasks: {}", e);
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

        // Wait for events
        debug!("Waiting for service events");
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    match event {
                        ServiceEvent::Shutdown => {
                            info!("Received shutdown event, stopping...");
                            break;
                        },
                        ServiceEvent::SessionChange(param) => {
                            let session_id = param.notification.session_id;
                            debug!("Processing session change event: {:?}, session ID: {}", param.reason, session_id);

                            // Handle session change based on both the reason and session ID
                            // We only care about events for the session assigned to this process (assigned_session_id)

                            // First, check if this event is for our assigned session
                            if service_state.assigned_session_id != Some(session_id) {
                                debug!("Event for session {} doesn't match assigned session {:?}, ignoring",
                                      session_id, service_state.assigned_session_id);
                                continue;
                            }

                            // Now handle events for our assigned session
                            match param.reason {
                                // For console connect, remote connect, and session logon events
                                // These events indicate our session is becoming active
                                windows_service::service::SessionChangeReason::ConsoleConnect |
                                windows_service::service::SessionChangeReason::RemoteConnect |
                                windows_service::service::SessionChangeReason::SessionLogon => {
                                    if !service_state.device_watch_handle.is_some() {
                                        info!("This session ({}) is becoming active, starting service tasks", session_id);
                                        if let Err(e) = service_state.start_service().await {
                                            error!("Failed to start service tasks: {}", e);
                                        }
                                    } else {
                                        info!("This session ({}) is becoming active, but service has been already
                                        started, ignoring...", session_id);
                                    }
                                },
                                // For session logoff events, we need to stop our service
                                windows_service::service::SessionChangeReason::SessionLogoff => {
                                    if service_state.device_watch_handle.is_some() {
                                        info!("This session ({}) is logging off, stopping service tasks", session_id);
                                        service_state.stop_service();
                                    } else {
                                        debug!("This session ({}) is logging off, but service is not started, can't \
                                        stop it, ignoring...", session_id)
                                    }
                                },
                                // For console disconnect events, we should stop our service
                                windows_service::service::SessionChangeReason::ConsoleDisconnect |
                                windows_service::service::SessionChangeReason::RemoteDisconnect => {
                                    if service_state.device_watch_handle.is_some() {
                                        info!("This session ({}) is disconnecting, stopping service tasks", session_id);
                                        service_state.stop_service();
                                    } else {
                                        debug!("This session ({}) is disconnecting, but service is not started, can't \
                                        stop it, ignoring...",
                                            session_id)
                                    }
                                },
                                // For other events, just log and continue
                                _ => {
                                    debug!("Received event {:?} for this session ({}), no action needed", param.reason,
                                        session_id);
                                    continue;
                                }
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to receive event: {}", e);
                }
            }
        }

        // Stop the service tasks
        debug!("Stopping service tasks");
        service_state.stop_service();

        info!("Exiting service");
    });

    // Tell the system that the service has stopped
    debug!("Setting service status to Stopped");
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