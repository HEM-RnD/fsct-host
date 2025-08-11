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

use std::sync::Arc;
use std::time::Duration;
use nusb::{list_devices, DeviceId, DeviceInfo};
use log::{debug, info, warn};
use nusb::hotplug::HotplugEvent;
use futures::StreamExt;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use crate::device_manager::{DeviceManagement, ManagedDeviceId};
use crate::usb::create_and_configure_fsct_device;
use crate::usb::errors::DeviceDiscoveryError;
use crate::usb::fsct_device::FsctDevice;

/// Handle for the USB device watch task
pub struct UsbDeviceWatchHandle {
    handle: JoinHandle<()>,
    shutdown_sender: oneshot::Sender<()>,
}

impl UsbDeviceWatchHandle {
    /// Creates a new UsbDeviceWatchHandle
    pub fn new(handle: JoinHandle<()>, shutdown_sender: oneshot::Sender<()>) -> Self {
        Self {
            handle,
            shutdown_sender,
        }
    }

    /// Shuts down the USB device watch task
    pub async fn shutdown(self) -> Result<(), tokio::task::JoinError> {
        let _ = self.shutdown_sender.send(());
        self.handle.await
    }

    /// Aborts the USB device watch task
    pub fn abort(self) {
        self.handle.abort();
    }
}

/// Tries to initialize a device and add it to the device manager
async fn try_initialize_device_and_add_to_manager<T: DeviceManagement>(
    device_info: &DeviceInfo,
    device_manager: &T,
) -> Result<ManagedDeviceId, DeviceDiscoveryError> {
    let device = create_and_configure_fsct_device(device_info).await?;

    // Enable the device
    device.set_enable(true).await?;

    // Add to device manager
    let managed_id = device_manager.add_device(Arc::new(device), device_info);

    Ok(managed_id)
}

/// Gets device info by device ID
async fn get_device_info_by_id(device_id: DeviceId) -> Option<nusb::DeviceInfo> {
    list_devices().ok()?.find(|device| device.id() == device_id)
}

/// Runs device initialization in a separate task
async fn run_device_initialization<T: DeviceManagement + Send + Sync + 'static>(
    device_info: DeviceInfo,
    device_manager: Arc<T>,
) {
    tokio::spawn(async move {
        let retry_timeout = Duration::from_secs(3);
        let retry_period = Duration::from_millis(100);
        let retry_timout_timepoint = std::time::Instant::now() + retry_timeout;

        let mut result = None;

        while std::time::Instant::now() < retry_timout_timepoint {
            if let Some(device_info) = get_device_info_by_id(device_info.id()).await {
                let res = try_initialize_device_and_add_to_manager(&device_info, device_manager.as_ref()).await;
                match res {
                    Ok(managed_id) => {
                        result = Some(Ok(managed_id));
                        break;
                    }
                    Err(DeviceDiscoveryError::Or(_)) => {
                        result = Some(Err(res.unwrap_err()));
                        break;
                    }
                    Err(DeviceDiscoveryError::ProtocolVersionNotSupported(_)) => {
                        result = Some(Err(res.unwrap_err()));
                        break;
                    }
                    _ => ()
                }
            }
            tokio::time::sleep(retry_period).await;
        }

        log_device_initialize_result(result, &device_info);
    });
}

/// Logs the result of device initialization
fn log_device_initialize_result(
    result: Option<Result<ManagedDeviceId, DeviceDiscoveryError>>, 
    device_info: &DeviceInfo
) {
    match result {
        Some(Ok(_)) => info!("Device with Ferrum Streaming Control Technology capability found: \"{}\" ({:04X}:{:04X})",
                          device_info.product_string().unwrap_or("Unknown"),
                          device_info.vendor_id(),
                          device_info.product_id()),
        Some(Err(e)) => warn!("Failed to initialize device {:04x}:{:04x}: {}", 
                           device_info.vendor_id(),
                           device_info.product_id(), 
                           e),
        None => warn!("Failed to initialize device {:04x}:{:04x}: Timeout", 
                   device_info.vendor_id(),
                   device_info.product_id()),
    }
}

/// Deinitializes all devices in the device manager
async fn deinitialize_devices<T: DeviceManagement>(device_manager: &T) {
    // Get all devices
    let devices = device_manager.remove_all_devices();
    for (id, device) in devices {
        let res = device.set_enable(false).await;
        if let Err(e) = res {
            warn!("Failed to disable device {}: {}", id, e); 
        }
    }
}

/// Runs the USB device watch task
pub async fn run_usb_device_watch<T: DeviceManagement + Send + Sync + 'static>(
    device_manager: Arc<T>,
) -> Result<UsbDeviceWatchHandle, anyhow::Error> {
    let mut devices_plug_events_stream = nusb::watch_devices()?;
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();

    let join_handle = tokio::spawn(async move {
        // Initialize existing devices
        let devices = list_devices().unwrap();
        for device_info in devices {
            let res = try_initialize_device_and_add_to_manager(&device_info, &*device_manager).await;
            log_device_initialize_result(Some(res), &device_info);
        }

        // Process events until shutdown is requested or stream ends
        let mut shutdown_future = shutdown_receiver;

        loop {
            // Use tokio::select! to wait for either a device event or shutdown signal
            tokio::select! {
                maybe_event = devices_plug_events_stream.next() => {
                    match maybe_event {
                        Some(event) => {
                            match event {
                                HotplugEvent::Connected(device_info) => {
                                    run_device_initialization(
                                        device_info, 
                                        device_manager.clone(),
                                    ).await;
                                }
                                HotplugEvent::Disconnected(device_id) => {
                                    // Remove the device from the manager
                                    if let Some(removed_device) = device_manager.remove_device_by_usb_id(device_id) {
                                        drop(removed_device);
                                        info!("FSCT Device removed");
                                    }
                                }
                            }
                        },
                        None => {
                            // Stream ended
                            debug!("Device events stream ended");
                            break;
                        }
                    }
                },
                _ = &mut shutdown_future => {
                    debug!("Shutdown requested, stopping USB device watch task");
                    deinitialize_devices(&*device_manager).await;
                    break;
                }
            }
        }
    });

    Ok(UsbDeviceWatchHandle::new(join_handle, shutdown_sender))
}