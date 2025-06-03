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
    },
    service_manager::{ServiceManager, ServiceManagerAccess},
};
use std::path::PathBuf;
use anyhow::Result;
use log::{info, error};

pub const SERVICE_NAME: &str = "FsctNativeService";
pub const SERVICE_DISPLAY_NAME: &str = "FSCT Native Service";
pub const SERVICE_DESCRIPTION: &str = "Ferrum Streaming Control Technology Native Service";

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
        service_type: ServiceType::USER_OWN_PROCESS,
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
