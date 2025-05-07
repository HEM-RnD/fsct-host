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

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use nusb::{list_devices, DeviceId, DeviceInfo};
use std::time::Duration;
use async_trait::async_trait;
use log::{debug, info, warn, error};
use nusb::hotplug::HotplugEvent;
use futures::StreamExt;
use crate::player::{PlayerEvent, PlayerState};
use crate::player_watch::PlayerEventListener;
use crate::usb::create_and_configure_fsct_device;
use crate::usb::errors::{DeviceDiscoveryError};
use crate::usb::fsct_device::FsctDevice;

pub type DeviceMap = Arc<Mutex<HashMap<DeviceId, Arc<FsctDevice>>>>;

async fn try_initialize_device(device_info: &DeviceInfo) -> Result<FsctDevice, DeviceDiscoveryError>
{
    let fsct_device = create_and_configure_fsct_device(device_info).await?;

    let time_diff = fsct_device.time_diff();
    debug!("Time difference: {:?}", time_diff);

    let enable = fsct_device.get_enable().await?;
    debug!("Enable: {}", enable);

    if !enable {
        debug!("Enabling FSCT...");
        fsct_device.set_enable(true).await?;
        let enable = fsct_device.get_enable().await?;
        debug!("Enable: {}", enable);
    } else {
        debug!("FSCT is already enabled.");
    }
    Ok(fsct_device)
}

async fn try_initialize_device_and_add_to_list(device_info: &DeviceInfo,
                                               devices: &DeviceMap,
                                               current_state: &Mutex<PlayerState>)
    -> Result<(), DeviceDiscoveryError>
{
    let fsct_device = try_initialize_device(device_info).await?;

    let current_state = current_state.lock().unwrap().clone();
    apply_player_state_on_device(&fsct_device, &current_state).await?;

    let mut fsct_devices = devices.lock().unwrap();
    let device_id = device_info.id();
    if fsct_devices.contains_key(&device_id) {
        warn!("Device {:04x}:{:04x} is already in the list.", device_info.vendor_id(), device_info.product_id());
        return Ok(());
    }
    fsct_devices.insert(device_id, Arc::new(fsct_device));
    Ok(())
}

async fn get_device_info_by_id(device_id: DeviceId) -> Option<nusb::DeviceInfo>
{
    list_devices().ok()?.find(|device| device.id() == device_id)
}

async fn run_device_initialization(device_info: DeviceInfo,
                                   devices: DeviceMap,
                                   current_metadata: Arc<Mutex<PlayerState>>)
{
    tokio::spawn(async move {
        let retry_timeout = Duration::from_secs(3);
        let retry_period = Duration::from_millis(100);
        let retry_timout_timepoint = std::time::Instant::now() + retry_timeout;

        let mut res = Ok(());

        while std::time::Instant::now() < retry_timout_timepoint {
            if let Some(device_info) = get_device_info_by_id(device_info.id()).await {
                res = try_initialize_device_and_add_to_list(&device_info, &devices, &current_metadata).await;
                match res {
                    Ok(_) => break,
                    Err(DeviceDiscoveryError::Or(_)) => break,
                    Err(DeviceDiscoveryError::ProtocolVersionNotSupported(_)) => break,
                    _ => ()
                }
            }
            tokio::time::sleep(retry_period).await;
        }
        log_device_initialize_result(res, &device_info);
    });
}

async fn apply_event_on_device(fsct_device: &FsctDevice, event: &PlayerEvent) -> anyhow::Result<()> {
    match event {
        PlayerEvent::StatusChanged(status) => fsct_device.set_status(status.clone()).await?,
        PlayerEvent::TimelineChanged(timeline) => fsct_device.set_progress(timeline.clone()).await?,
        PlayerEvent::TextChanged((text_id, text)) => fsct_device.set_current_text(text_id.clone(), text.as_ref().map(|s| s.as_str())).await?
    }
    Ok(())
}

async fn apply_player_state_on_device(device: &FsctDevice,
                                      current_state: &PlayerState) -> anyhow::Result<()> {
    apply_event_on_device(device, &PlayerEvent::StatusChanged(current_state.status.clone())).await?;
    apply_event_on_device(device, &PlayerEvent::TimelineChanged(current_state.timeline.clone())).await?;
    for (text_id, text) in current_state.texts.iter() {
        apply_event_on_device(device, &PlayerEvent::TextChanged((text_id, text.clone()))).await?;
    }
    Ok(())
}

fn log_device_initialize_result(result: Result<(), DeviceDiscoveryError>, device_info: &DeviceInfo) {
    match result {
        Ok(_) => info!("Device with Ferrum Streaming Control Technology capability found: \"{}\" ({:04X}:{:04X})",
                      device_info.product_string().unwrap_or("Unknown"),
                      device_info.vendor_id(),
                      device_info.product_id()),
        Err(e) => warn!("Failed to initialize device {:04x}:{:04x}: {}", device_info.vendor_id(),
                      device_info.product_id(), e),
    }
}

pub async fn run_devices_watch(fsct_devices: DeviceMap, current_metadata: Arc<Mutex<PlayerState>>)
    -> Result<tokio::task::JoinHandle<()>, anyhow::Error>
{
    let mut devices_plug_events_stream = nusb::watch_devices()?;
    let join_handle = tokio::spawn(async move {
        let devices = list_devices().unwrap();
        for device_info in devices {
            let res = try_initialize_device_and_add_to_list(&device_info, &fsct_devices, &current_metadata).await;
            log_device_initialize_result(res, &device_info);
        }
        while let Some(event) = devices_plug_events_stream.next().await {
            match event {
                HotplugEvent::Connected(device_info) => {
                    run_device_initialization(device_info.clone(), fsct_devices.clone(), current_metadata.clone()).await;
                }
                HotplugEvent::Disconnected(device_id) => {
                    let mut fsct_devices = fsct_devices.lock().unwrap();
                    fsct_devices.remove(&device_id);
                }
            }
        }
    });
    Ok(join_handle)
}

pub struct DevicesPlayerEventApplier {
    device_map: DeviceMap,
}

impl DevicesPlayerEventApplier {
    pub fn new(device_map: DeviceMap) -> Self {
        Self {
            device_map,
        }
    }
}

#[async_trait]
impl PlayerEventListener for DevicesPlayerEventApplier {
    async fn on_event(&self, event: PlayerEvent) {
        let devices = self.device_map.lock().unwrap().values().cloned().collect::<Vec<_>>();
        for device in devices {
            let result = apply_event_on_device(&device, &event).await;
            if let Err(e) = result {
                error!("Failed to apply changes on device: {}", e);
            }
        }
    }
}