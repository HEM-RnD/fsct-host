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
use std::path::PathBuf;
use anyhow::Result;
use log::{info, error, debug};
use windows_service::{
    service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
    },
    service_manager::{ServiceManager, ServiceManagerAccess},
};
use crate::windows::service::cli::LogLevel;
use crate::windows::service::constants::{SERVICE_NAME, SERVICE_DISPLAY_NAME, SERVICE_DESCRIPTION};

fn get_service_type(user_service: bool) -> ServiceType
{
    if user_service {
        ServiceType::USER_OWN_PROCESS
    } else {
        ServiceType::OWN_PROCESS
    }
}

pub fn install_service(log_level: Option<LogLevel>, user_service: bool) -> Result<()> {
    debug!("Starting service installation");

    debug!("Connecting to service manager");
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = match ServiceManager::local_computer(None::<&str>, manager_access) {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to connect to service manager: {}", e);
            return Err(e.into());
        }
    };

    // Get the current executable path
    debug!("Getting current executable path");
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

    debug!("Service binary path: {}", service_binary_path);
    let mut launch_arguments =  vec![];
    if let Some(log_level) = log_level {
        launch_arguments.extend_from_slice(&[OsString::from("--log-level"), OsString::from(log_level.to_string())])
    };
    launch_arguments.extend_from_slice(&[OsString::from("service"), OsString::from("run")]);

    // Create the service info
    debug!("Creating service info");
    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: get_service_type(user_service),
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: PathBuf::from(service_binary_path),
        launch_arguments,
        dependencies: vec![],
        account_name: None, // Run as LocalSystem
        account_password: None,
    };

    // Create the service
    debug!("Creating service");
    let service = match service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG) {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to create service: {}", e);
            return Err(e.into());
        }
    };

    // Set the service description
    debug!("Setting service description");
    if let Err(e) = service.set_description(SERVICE_DESCRIPTION) {
        error!("Failed to set service description: {}", e);
        return Err(e.into());
    }

    info!("Service installed successfully");
    println!("Service installed successfully");
    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    debug!("Starting service uninstallation");

    debug!("Connecting to service manager");
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = match ServiceManager::local_computer(None::<&str>, manager_access) {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to connect to service manager: {}", e);
            return Err(e.into());
        }
    };

    debug!("Opening service: {}", SERVICE_NAME);
    let service_access = ServiceAccess::DELETE;
    let service = match service_manager.open_service(SERVICE_NAME, service_access) {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to open service: {}", e);
            return Err(e.into());
        }
    };

    // Delete the service
    debug!("Deleting service");
    if let Err(e) = service.delete() {
        error!("Failed to delete service: {}", e);
        return Err(e.into());
    }
    info!("Service uninstalled successfully");
    println!("Service uninstalled successfully");
    Ok(())
}