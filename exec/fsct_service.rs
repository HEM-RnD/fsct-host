use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use futures::StreamExt;
use log::error;
// use tokio::main;
use dac_player_integration::usb::create_and_configure_fsct_device;
use nusb::{list_devices, DeviceId, DeviceInfo};
use nusb::hotplug::HotplugEvent;
use dac_player_integration::platform::{TimelineInfo, Track};
use dac_player_integration::usb::definitions::FsctTextMetadata;
use dac_player_integration::usb::requests::FsctStatus;
use dac_player_integration::platform;
use dac_player_integration::usb::fsct_device::FsctDevice;

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
    return Ok(fsct_device);
}

async fn try_initialize_device_and_add_to_list(device_info: &DeviceInfo, devices: &DeviceMap)
{
    let fsct_device = try_initialize_device(device_info).await;
    match fsct_device {
        Ok(device) => {
            let mut fsct_devices = devices.lock().unwrap();
            let device_id = device_info.id();
            fsct_devices.insert(device_id, Arc::new(device));
        }
        Err(error_string) => {
            println!("Device {:04x}:{:04x} omitted: {}", device_info.vendor_id(), device_info.product_id(), error_string);
        }
    }
}

fn run_devices_watch(fsct_devices: DeviceMap)
{
    tokio::spawn(async move {
        let mut devices_plug_events_stream = nusb::watch_devices().unwrap();
        let devices = list_devices().unwrap();
        for device in devices {
            try_initialize_device_and_add_to_list(&device, &fsct_devices).await;
        }
        while let Some(event) = devices_plug_events_stream.next().await {
            match event {
                HotplugEvent::Connected(device_info) => {
                    try_initialize_device_and_add_to_list(&device_info, &fsct_devices).await;
                }
                HotplugEvent::Disconnected(device_id) => {
                    let mut fsct_devices = fsct_devices.lock().unwrap();
                    fsct_devices.remove(&device_id);
                }
            }
        }
    });
}

struct Changes {
    current_track: Option<Option<Track>>,
    timeline_info: Option<Option<TimelineInfo>>,
    status: Option<FsctStatus>,
}


fn log_changes(changes: &Changes)
{
    if let Some(current_track) = changes.current_track.as_ref() {
        println!("Current track: {:?}", current_track);
    }
    if let Some(timeline_info) = changes.timeline_info.as_ref() {
        println!("Timeline info: {:?}", timeline_info);
    }
    if let Some(status) = changes.status {
        println!("Status: {:?}", status);
    }
}

fn run_platform_watch(fsct_devices: DeviceMap, platform_context: platform::PlatformContext)
{
    tokio::spawn(async move {
        let mut last_current_track: Option<Track> = None;
        let mut last_timeline_info: Option<TimelineInfo> = None;
        let mut last_status = FsctStatus::Unknown;

        loop {
            let mut changes = Changes {
                current_track: None,
                timeline_info: None,
                status: None,
            };

            let new_current_track = platform_context.info.get_current_track().await.ok();
            if new_current_track != last_current_track {
                changes.current_track = Some(new_current_track.clone());
                last_current_track = new_current_track;
            }

            let new_timeline_info = platform_context.info.get_timeline_info().await.ok().flatten();
            if new_timeline_info != last_timeline_info {
                changes.timeline_info = Some(new_timeline_info.clone());
                last_timeline_info = new_timeline_info;
            }

            let new_status_result = platform_context.info.is_playing().await;
            let new_status = match new_status_result {
                Ok(true) => FsctStatus::Playing,
                Ok(false) => FsctStatus::Paused,
                Err(_) => FsctStatus::Unknown,
            };

            if new_status != last_status {
                changes.status = Some(new_status);
                last_status = new_status;
            }

            log_changes(&changes);
            apply_changes_on_devices(&fsct_devices, changes).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

async fn apply_changes_on_device(device: &FsctDevice, changes: &Changes) -> Result<(), String>
{
    if let Some(current_track) = changes.current_track.as_ref() {
        let (current_title, current_artist)
            = current_track.as_ref()
                           .map(|track| (track.title.as_str(), track.artist.as_str()))
                           .unzip();
        device.set_current_text(FsctTextMetadata::CurrentAuthor, current_artist).await?;
        device.set_current_text(FsctTextMetadata::CurrentTitle, current_title).await?;
    }
    if let Some(timeline_info) = changes.timeline_info.as_ref() {
        device.set_progress(timeline_info.clone()).await?;
    }
    if let Some(status) = changes.status {
        device.set_status(status).await?;
    }
    Ok(())
}

async fn apply_changes_on_devices(devices: &DeviceMap, changes: Changes) {
    let devices = devices.lock().unwrap().values().cloned().collect::<Vec<_>>();
    for device in devices {
        let result = apply_changes_on_device(&device, &changes).await;
        if let Err(e) = result {
            error!("Failed to apply changes on device: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let platform = platform::get_platform();
    let platform_context = platform.initialize().await?;
    let fsct_devices = Arc::new(Mutex::new(HashMap::new()));
    run_devices_watch(fsct_devices.clone());
    run_platform_watch(fsct_devices.clone(), platform_context);

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C signal");
    println!("Exiting...");
    Ok(())
}