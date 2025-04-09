use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use nusb::{list_devices, DeviceId, DeviceInfo};
use std::time::Duration;
use async_trait::async_trait;
use log::error;
use nusb::hotplug::HotplugEvent;
use futures::StreamExt;
use crate::player::{PlayerEvent, PlayerState};
use crate::player_watch::PlayerEventListener;
use crate::usb::create_and_configure_fsct_device;
use crate::usb::fsct_device::FsctDevice;

type DeviceMap = Arc<Mutex<HashMap<DeviceId, Arc<FsctDevice>>>>;

async fn try_initialize_device(device_info: &DeviceInfo) -> Result<FsctDevice, String>
{
    let fsct_device = create_and_configure_fsct_device(device_info).await?;

    println!("Device with Ferrum Streaming Control Technology capability found: \"{}\" ({:04X}:{:04X})",
             device_info.product_string().unwrap_or("Unknown"),
             device_info.vendor_id(),
             device_info.product_id());

    let time_diff = fsct_device.time_diff();
    println!("Time difference: {:?}", time_diff);

    let enable = fsct_device.get_enable().await?;
    println!("Enable: {}", enable);

    if !enable {
        println!("Enabling FSCT...");
        fsct_device.set_enable(true).await?;
        let enable = fsct_device.get_enable().await?;
        println!("Enable: {}", enable);
    } else {
        println!("FSCT is already enabled.");
    }
    Ok(fsct_device)
}

async fn try_initialize_device_and_add_to_list(device_info: &DeviceInfo,
                                               devices: &DeviceMap,
                                               current_state: &Mutex<PlayerState>)
    -> Result<(), String>
{
    let fsct_device = match try_initialize_device(device_info).await {
        Ok(fsct_device) => fsct_device,
        Err(e) => {
            println!("Failed to initialize device {:04x}:{:04x}: {}", device_info.vendor_id(),
                     device_info.product_id(), e);
            return Err(e);
        }
    };

    let current_state = current_state.lock().unwrap().clone();
    apply_player_state_on_device(&fsct_device, &current_state).await?;

    let mut fsct_devices = devices.lock().unwrap();
    let device_id = device_info.id();
    if fsct_devices.contains_key(&device_id) {
        println!("Device {:04x}:{:04x} is already in the list.", device_info.vendor_id(), device_info
            .product_id());
        return Ok(());
    }
    fsct_devices.insert(device_id, Arc::new(fsct_device));
    Ok(())
}

async fn get_device_info_by_id(device_id: DeviceId) -> Option<nusb::DeviceInfo>
{
    match nusb::list_devices() {
        Ok(mut list) => list.find(|device| device.id() == device_id),
        Err(_) => return None
    }
}

async fn run_device_initialization(device_info: DeviceInfo,
                                   devices: DeviceMap,
                                   current_metadata: Arc<Mutex<PlayerState>>)
{
    tokio::spawn(async move {
        let retry_timeout = Duration::from_secs(3);
        let retry_period = Duration::from_millis(100);
        let retry_timout_timepoint = std::time::Instant::now() + retry_timeout;

        while std::time::Instant::now() < retry_timout_timepoint {
            if let Some(device_info) = get_device_info_by_id(device_info.id()).await {
                //todo distinguish access problems from lack of FSCT features!!!

                let res = try_initialize_device_and_add_to_list(&device_info, &devices, &current_metadata).await;
                if res.is_ok() {
                    return;
                }
            }
            tokio::time::sleep(retry_period).await;
        }
        println!("Device {:04x}:{:04x} omitted after many retries.", device_info.vendor_id(), device_info
            .product_id());
    });
}

async fn apply_event_on_device(fsct_device: &FsctDevice, event: &PlayerEvent) -> Result<(), String> {
    match event {
        PlayerEvent::StatusChanged(status) => fsct_device.set_status(status.clone()).await?,
        PlayerEvent::TimelineChanged(timeline) => fsct_device.set_progress(timeline.clone()).await?,
        PlayerEvent::TextChanged((text_id, text)) => fsct_device.set_current_text(text_id.clone(), text.as_ref().map(|s| s.as_str())).await?
    }
    Ok(())
}

async fn apply_player_state_on_device(device: &FsctDevice,
                                      current_state: &PlayerState) -> Result<(), String> {
    apply_event_on_device(device, &PlayerEvent::StatusChanged(current_state.status.clone())).await?;
    apply_event_on_device(device, &PlayerEvent::TimelineChanged(current_state.timeline.clone())).await?;
    for (text_id, text) in current_state.texts.iter() {
        apply_event_on_device(device, &PlayerEvent::TextChanged((text_id, text.clone()))).await?;
    }
    Ok(())
}

pub async fn run_devices_watch(fsct_devices: DeviceMap, current_metadata: Arc<Mutex<PlayerState>>) -> Result<(), String>
{
    let mut devices_plug_events_stream = nusb::watch_devices().map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let devices = list_devices().unwrap();
        for device in devices {
            let _ = try_initialize_device_and_add_to_list(&device, &fsct_devices, &current_metadata).await;
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
    Ok(())
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