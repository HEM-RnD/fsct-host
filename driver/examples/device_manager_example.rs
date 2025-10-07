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
use anyhow::Result;
use fsct_driver::{
    DeviceManager, DeviceManagement, DeviceControl,
    run_usb_device_watch, DeviceEvent
};
use fsct::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use log::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();
    info!("Starting device manager example");

    // Create device manager
    let device_manager = Arc::new(DeviceManager::new());
    info!("Device manager created");

    // Subscribe to device events
    let mut device_events = device_manager.subscribe();
    
    // Start a task to handle device events
    let event_task = tokio::spawn(async move {
        while let Ok(event) = device_events.recv().await {
            match event {
                DeviceEvent::Added(device_id) => {
                    info!("Device added with managed ID: {}", device_id);
                },
                DeviceEvent::Removed(device_id) => {
                    info!("Device removed with managed ID: {}", device_id);
                }
            }
        }
    });

    // Start watching for USB devices
    info!("Starting USB device watch");
    let device_watch_handle = run_usb_device_watch(
        device_manager.clone(),
    ).await?;

    // Wait for devices to be discovered
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Get all discovered devices with detailed info
    let detected_devices = device_manager.get_detected_devices();
    info!("Discovered {} devices", detected_devices.len());

    // Print detailed information about each detected device
    for device_info in &detected_devices {
        info!("Device details:");
        info!("  ID: {}", device_info.id);
        info!("  Name: {}", device_info.name.as_deref().unwrap_or("N/A"));
        info!("  Manufacturer: {}", device_info.manufacturer.as_deref().unwrap_or("N/A"));
        info!("  VID: 0x{:04X}", device_info.vendor_id);
        info!("  PID: 0x{:04X}", device_info.product_id);
        info!("  Serial: {}", device_info.serial_number.as_deref().unwrap_or("N/A"));
    }

    // Get all discovered device IDs
    let devices = device_manager.get_all_managed_ids();
    info!("Device IDs: {:?}", devices);

    // Interact with each device
    for managed_id in &devices {
        info!("Setting status for device {}", managed_id);
        if let Err(e) = device_manager.set_status(*managed_id, FsctStatus::Playing).await {
            warn!("Failed to set status for device {}: {}", managed_id, e);
        }

        info!("Setting text for device {}", managed_id);
        if let Err(e) = device_manager.set_current_text(
            *managed_id,
            FsctTextMetadata::CurrentTitle,
            Some("Example Song Title"),
        ).await {
            warn!("Failed to set text for device {}: {}", managed_id, e);
        }

        // Create a progress object
        let progress = TimelineInfo {
            position: Duration::from_secs(30),
            duration: Duration::from_secs(180),
            rate: 1.0,
            update_time: std::time::SystemTime::now(),
        };

        info!("Setting progress for device {}", managed_id);
        if let Err(e) = device_manager.set_progress(*managed_id, Some(progress)).await {
            warn!("Failed to set progress for device {}: {}", managed_id, e);
        }
    }

    // Wait for a while to observe the devices
    info!("Waiting for 10 seconds...");
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Shutdown device watching
    info!("Shutting down USB device watch");
    device_watch_handle.shutdown().await?;
    
    // Abort the event handling task
    event_task.abort();

    info!("Example completed successfully");
    Ok(())
}