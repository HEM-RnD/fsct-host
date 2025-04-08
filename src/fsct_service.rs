use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use futures::StreamExt;
use log::error;
// use tokio::main;
use dac_player_integration::usb::create_and_configure_fsct_device;
use nusb::{list_devices, DeviceId, DeviceInfo};
use nusb::hotplug::HotplugEvent;
use dac_player_integration::platform::{PlatformContext, TimelineInfo, Track};
use dac_player_integration::definitions::FsctTextMetadata;
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
    Ok(fsct_device)
}

async fn try_initialize_device_and_add_to_list(device_info: &DeviceInfo,
                                               devices: &DeviceMap,
                                               current_metadata: &Mutex<CurrentMetadata>)
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

    apply_changes_on_device(&fsct_device, &current_metadata, &Changes {
        current_track: true,
        status: true,
        timeline_info: true,
    }).await?;

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
                                   current_metadata: Arc<Mutex<CurrentMetadata>>)
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

fn run_devices_watch(fsct_devices: DeviceMap, current_metadata: Arc<Mutex<CurrentMetadata>>)
{
    tokio::spawn(async move {
        let mut devices_plug_events_stream = nusb::watch_devices().unwrap();
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
}


struct CurrentMetadata {
    current_track: Option<Track>,
    timeline_info: Option<TimelineInfo>,
    status: FsctStatus,
}

struct Changes {
    current_track: bool,
    timeline_info: bool,
    status: bool,
}


fn log_changes(changes: &Changes, current_metadata: &CurrentMetadata)
{
    if changes.current_track {
        println!("Current track: {:?}", current_metadata.current_track);
    }
    if changes.timeline_info {
        println!("Timeline info: {:?}", current_metadata.timeline_info);
    }
    if changes.status {
        println!("Status: {:?}", current_metadata.status);
    }
}

async fn update_current_metadata(platform_context: &PlatformContext,
                                 current_metadata: &Mutex<CurrentMetadata>) -> Changes
{
    let mut changes = Changes {
        current_track: false,
        timeline_info: false,
        status: false,
    };

    let new_current_track = platform_context.info.get_current_track().await.ok();
    let new_timeline_info = platform_context.info.get_timeline_info().await.ok().flatten();
    let new_status_result = platform_context.info.is_playing().await;

    let mut current_metadata = current_metadata.lock().unwrap();
    if new_current_track != current_metadata.current_track {
        changes.current_track = true;
        current_metadata.current_track = new_current_track;
    }

    if new_timeline_info != current_metadata.timeline_info {
        changes.timeline_info = true;
        current_metadata.timeline_info = new_timeline_info;
    }

    let new_status = match new_status_result {
        Ok(true) => FsctStatus::Playing,
        Ok(false) => FsctStatus::Paused,
        Err(_) => FsctStatus::Unknown,
    };

    if new_status != current_metadata.status {
        changes.status = true;
        current_metadata.status = new_status;
    }

    log_changes(&changes, &current_metadata);

    changes
}

fn run_metadata_watch(fsct_devices: DeviceMap,
                      platform_context: PlatformContext,
                      current_metadata: Arc<Mutex<CurrentMetadata>>)
{
    tokio::spawn(async move {
        loop {
            let changes = update_current_metadata(&platform_context, &current_metadata).await;
            apply_changes_on_devices(&fsct_devices, &current_metadata, changes).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

async fn apply_changes_on_device(device: &FsctDevice, current_metadata: &Mutex<CurrentMetadata>, changes: &Changes)
    -> Result<
        (), String>
{
    if changes.current_track {
        let (current_title, current_artist)
            = current_metadata.lock().unwrap()
                              .current_track
                              .as_ref()
                              .map(|track| (track.title.clone(), track.artist.clone()))
                              .unzip();
        let current_title = current_title.as_ref().map(|v| v.as_str());
        let current_artist = current_artist.as_ref().map(|v| v.as_str());

        device.set_current_text(FsctTextMetadata::CurrentAuthor, current_artist).await?;
        device.set_current_text(FsctTextMetadata::CurrentTitle, current_title).await?;
    }
    if changes.timeline_info {
        let timeline_info = current_metadata.lock().unwrap().timeline_info.clone();
        device.set_progress(timeline_info).await?;
    }
    if changes.status {
        let status = current_metadata.lock().unwrap().status.clone();
        device.set_status(status).await?;
    }
    Ok(())
}

async fn apply_changes_on_devices(devices: &DeviceMap,
                                  current_metadata: &Mutex<CurrentMetadata>,
                                  changes: Changes) {
    let devices = devices.lock().unwrap().values().cloned().collect::<Vec<_>>();
    for device in devices {
        let result = apply_changes_on_device(&device, &current_metadata, &changes).await;
        if let Err(e) = result {
            error!("Failed to apply changes on device: {}", e);
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let platform = platform::get_platform();
    let platform_context = platform.initialize().await?;
    let fsct_devices = Arc::new(Mutex::new(HashMap::new()));
    let current_metadata = Arc::new(Mutex::new(CurrentMetadata {
        current_track: None,
        timeline_info: None,
        status: FsctStatus::Unknown,
    }));
    run_devices_watch(fsct_devices.clone(), current_metadata.clone());
    run_metadata_watch(fsct_devices.clone(), platform_context, current_metadata);

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C signal");
    println!("Exiting...");
    Ok(())
}