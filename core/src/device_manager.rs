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

use std::collections::HashMap;
use std::mem::swap;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use nusb::{DeviceId, DeviceInfo};
use tokio::sync::broadcast;
use thiserror::Error;
use uuid::Uuid;
use crate::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use crate::usb::errors::FsctDeviceError;
use crate::usb::fsct_device::FsctDevice;
use crate::device_uuid_calculator::calculate_uuid;

/// Unique identifier for managed devices
pub type ManagedDeviceId = Uuid;

/// Device event types that can be broadcast by the DeviceManager
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    /// A device was added with the given managed ID
    Added(ManagedDeviceId),
    /// A device was removed with the given managed ID
    Removed(ManagedDeviceId),
}

/// Error type for device manager operations
#[derive(Error, Debug)]
pub enum DeviceManagerError {
    /// The device with the specified ID was not found
    #[error("Device with ID {0} not found")]
    DeviceNotFound(ManagedDeviceId),
    
    /// An error occurred in the underlying FSCT device
    #[error("FSCT device error: {0}")]
    FsctDeviceError(#[from] FsctDeviceError),
}

/// Trait for device management operations
pub trait DeviceManagement {
    /// Add a device to the manager and return its managed ID
    fn add_device(&self, device: Arc<FsctDevice>, device_info: &DeviceInfo) -> ManagedDeviceId;
    
    /// Remove a device from the manager by its USB device ID
    fn remove_device_by_usb_id(&self, device_id: DeviceId) -> Option<Arc<FsctDevice>>;

    /// Remove all managed devices
    fn remove_all_devices(&self) -> Vec<(ManagedDeviceId, Arc<FsctDevice>)>;

    /// Get the managed ID for a USB device ID
    fn get_managed_id_for_usb_id(&self, device_id: DeviceId) -> Option<ManagedDeviceId>;

    /// Get all devices managed ID
    fn get_all_managed_ids(&self) -> Vec<ManagedDeviceId>;

}

/// Trait for device control operations
pub trait DeviceControl {
    /// Set the enable state for a device
    fn set_enable(&self, managed_id: ManagedDeviceId, enable: bool) -> impl std::future::Future<Output = Result<(), DeviceManagerError>> + Send + Sync;
    
    /// Get the enable state for a device
    fn get_enable(&self, managed_id: ManagedDeviceId) -> impl std::future::Future<Output = Result<bool, DeviceManagerError>> + Send + Sync;
    
    /// Set the progress for a device
    fn set_progress(&self, managed_id: ManagedDeviceId, progress: Option<TimelineInfo>) -> impl std::future::Future<Output = Result<(), DeviceManagerError>> + Send + Sync;
    
    /// Set text for a device
    fn set_current_text(&self, managed_id: ManagedDeviceId, text_id: FsctTextMetadata, text: Option<&str>) -> impl std::future::Future<Output = Result<(), DeviceManagerError>> + Send + Sync;
    
    /// Set status for a device
    fn set_status(&self, managed_id: ManagedDeviceId, status: FsctStatus) -> impl std::future::Future<Output =Result<(), DeviceManagerError>> + Send + Sync;

    /// Subscribe to device events
    fn subscribe(&self) -> broadcast::Receiver<DeviceEvent>;
}

/// Device manager that handles device ID management and provides a unified API for device operations
pub struct DeviceManager {
    /// Map of managed device IDs to FSCT devices
    devices: Arc<Mutex<HashMap<ManagedDeviceId, Arc<FsctDevice>>>>,
    
    /// Map of USB device IDs to managed device IDs
    usb_id_to_managed_id: Arc<Mutex<HashMap<DeviceId, ManagedDeviceId>>>,
    
    /// Broadcast sender for device events
    event_sender: broadcast::Sender<DeviceEvent>,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        // Create a broadcast channel with a capacity of 100 events
        let (event_sender, _) = broadcast::channel(100);
        
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            usb_id_to_managed_id: Arc::new(Mutex::new(HashMap::new())),
            event_sender,
        }
    }

    fn get_device(&self, managed_id: ManagedDeviceId) -> Result<Arc<FsctDevice>, DeviceManagerError> {
        let devices = self.devices.lock().unwrap();
        devices.get(&managed_id).cloned().ok_or(DeviceManagerError::DeviceNotFound(managed_id))
    }
}

impl DeviceManagement for DeviceManager {
    fn add_device(&self, device: Arc<FsctDevice>, device_info: &DeviceInfo) -> ManagedDeviceId {
        // Compute UUID from VID, PID, and Serial Number
        let vid = device_info.vendor_id();
        let pid = device_info.product_id();
        let sn = device_info.serial_number().unwrap_or("");
        let managed_id = calculate_uuid(vid, pid, sn);
        
        // Add to devices map
        {
            let mut devices = self.devices.lock().unwrap();
            devices.insert(managed_id, device);
        }
        
        // Add to USB ID mapping
        {
            let mut usb_id_map = self.usb_id_to_managed_id.lock().unwrap();
            usb_id_map.insert(device_info.id(), managed_id);
        }
        
        // Broadcast device added event
        let _ = self.event_sender.send(DeviceEvent::Added(managed_id));
        
        managed_id
    }
    
    fn remove_device_by_usb_id(&self, device_id: DeviceId) -> Option<Arc<FsctDevice>> {
        // Get the managed ID
        let managed_id = {
            let usb_id_map = self.usb_id_to_managed_id.lock().unwrap();
            *usb_id_map.get(&device_id)?
        };
        
        // Remove from USB ID mapping
        {
            let mut usb_id_map = self.usb_id_to_managed_id.lock().unwrap();
            usb_id_map.remove(&device_id);
        }
        
        // Remove from devices map
        let device = {
            let mut devices = self.devices.lock().unwrap();
            devices.remove(&managed_id)
        };
        
        // Broadcast device removed event if a device was actually removed
        if device.is_some() {
            let _ = self.event_sender.send(DeviceEvent::Removed(managed_id));
        }
        
        device
    }

    fn remove_all_devices(&self) -> Vec<(ManagedDeviceId, Arc<FsctDevice>)> {
        let mut local_devices = HashMap::new();
        let mut devices = self.devices.lock().unwrap();
        swap(&mut local_devices, devices.deref_mut());
        local_devices.into_iter()
            .map(|(id, device)| (id, device))
            .collect()
    }

    fn get_managed_id_for_usb_id(&self, device_id: DeviceId) -> Option<ManagedDeviceId> {
        let usb_id_map = self.usb_id_to_managed_id.lock().unwrap();
        usb_id_map.get(&device_id).copied()
    }

    fn get_all_managed_ids(&self) -> Vec<ManagedDeviceId> {
        let devices = self.devices.lock().unwrap();
        devices.keys().copied().collect()
    }
}

impl DeviceControl for DeviceManager {
    async fn set_enable(&self, managed_id: ManagedDeviceId, enable: bool) -> Result<(), DeviceManagerError> {
        let device = self.get_device(managed_id)?;
        device.set_enable(enable).await.map_err(DeviceManagerError::from)
    }
    
    async fn get_enable(&self, managed_id: ManagedDeviceId) -> Result<bool, DeviceManagerError> {
        let device = self.get_device(managed_id)?;
        device.get_enable().await.map_err(DeviceManagerError::from)
    }
    
    async fn set_progress(&self, managed_id: ManagedDeviceId, progress: Option<TimelineInfo>) -> Result<(), DeviceManagerError> {
        let device = self.get_device(managed_id)?;
        device.set_progress(progress).await.map_err(DeviceManagerError::from)
    }
    
    async fn set_current_text(&self, managed_id: ManagedDeviceId, text_id: FsctTextMetadata, text: Option<&str>) -> Result<(), DeviceManagerError> {
        let device = self.get_device(managed_id)?;
        device.set_current_text(text_id, text).await.map_err(DeviceManagerError::from)
    }
    
    async fn set_status(&self, managed_id: ManagedDeviceId, status: FsctStatus) -> Result<(), DeviceManagerError> {
        let device = self.get_device(managed_id)?;
        device.set_status(status).await.map_err(DeviceManagerError::from)
    }


    fn subscribe(&self) -> broadcast::Receiver<DeviceEvent> {
        self.event_sender.subscribe()
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}