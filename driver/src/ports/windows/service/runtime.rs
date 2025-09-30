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
use std::time::Duration;
use anyhow::{anyhow, Result};
use clap::Parser;
use log::{info, error, debug};
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
use windows_service::service_control_handler::ServiceStatusHandle;
use crate::{ServiceStateListener, StopSignal};
use crate::service_main::async_main;
use crate::cli::Cli;
use crate::ports::windows::service::get_service_name;

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
struct WindowsServiceStopSignal {
    event_rx: tokio::sync::broadcast::Receiver<ServiceEvent>,
}

impl WindowsServiceStopSignal {
    pub fn new(event_rx: tokio::sync::broadcast::Receiver<ServiceEvent>) -> Self {
        Self { event_rx }
    }
}

impl StopSignal for WindowsServiceStopSignal {
    async fn wait(&mut self) -> anyhow::Result<()> {
        self.event_rx.recv().await
            .map(|_event| ())
            .map_err(|_e| anyhow!("Error in listening for service stop event"))
    }
}

#[derive(Clone)]
struct WindowsServiceStateNotifier {
    status_handle: ServiceStatusHandle,
    service_type: ServiceType,
}

impl WindowsServiceStateNotifier {
    pub fn new(status_handle: ServiceStatusHandle, service_type: ServiceType) -> Self {
        Self { status_handle, service_type }
    }

    fn get_service_status(&self, state: ServiceState) -> ServiceStatus {
        ServiceStatus {
            service_type: self.service_type,
            current_state: state,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        }
    }

    fn on_service_start_pending(&self) -> anyhow::Result<()> {
        debug!("Setting service status to StartPending");
        let status = self.get_service_status(ServiceState::StartPending);
        self.status_handle.set_service_status(status).map_err(|e| anyhow!(e))
    }

    fn on_service_stopped(&self, exit_code: u32) -> anyhow::Result<()> {
        debug!("Setting service status to Stopped");
        let mut status = self.get_service_status(ServiceState::Stopped);
        if exit_code != 0 {
            status.exit_code = ServiceExitCode::ServiceSpecific(exit_code);
        }
        self.status_handle.set_service_status(status).map_err(|e| anyhow!(e))
    }
}

impl ServiceStateListener for WindowsServiceStateNotifier {
    fn on_service_started(&self) -> anyhow::Result<()> {
        debug!("Setting service status to Running");
        let mut status = self.get_service_status(ServiceState::Running);
        status.controls_accepted = ServiceControlAccept::STOP;
        self.status_handle.set_service_status(status).map_err(|e| anyhow!(e))

    }
    fn on_service_stopping(&self) -> anyhow::Result<()> {
        debug!("Setting service status to StopPending");
        let status = self.get_service_status(ServiceState::StopPending);
        self.status_handle.set_service_status(status).map_err(|e| anyhow!(e))
    }
}

pub fn run_service_main(_arguments: Vec<OsString>) -> anyhow::Result<()> {
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

    let service_status_notifier = WindowsServiceStateNotifier::new(status_handle, service_type);

    // Tell the system that the service is starting
    debug!("Setting service status to StartPending");


    service_status_notifier.on_service_start_pending()?;

    let stop_signal = WindowsServiceStopSignal::new(event_tx.subscribe());

    let listener = service_status_notifier.clone();
    // Run the service in the Tokio runtime
    let res = rt.block_on(async move {
        async_main(stop_signal, listener).await
    });

    rt.shutdown_timeout(Duration::from_secs(10));
    debug!("Service tasks stopped, exiting");

    // Tell the system that the service has stopped
    debug!("Setting service status to Stopped");
    let status = if res.is_ok() { 0u32 } else { 1u32 };
    service_status_notifier.on_service_stopped(status)?;

    res
        .inspect(|_|info!("Service stopped successfully"))
        .inspect_err(|e|error!("Service stopped, because it's failed: {}", e))
}
