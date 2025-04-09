use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use futures::{SinkExt, StreamExt};
use futures::channel::mpsc::{SendError, Sender};
use log::error;
use crate::usb::create_and_configure_fsct_device;
use nusb::{list_devices, DeviceId, DeviceInfo};
use nusb::hotplug::HotplugEvent;
use crate::definitions::{FsctTextMetadata};
use crate::player::{Player, PlayerError, PlayerEvent, PlayerEventListener, PlayerInterface, PlayerState};
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

async fn run_devices_watch(fsct_devices: DeviceMap, current_metadata: Arc<Mutex<PlayerState>>) -> Result<(), String>
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

async fn update_current_status(new_state: &PlayerState, current_state: &mut PlayerState, tx: &mut
Sender<PlayerEvent>) -> Result<(), SendError> {
    if new_state.status != current_state.status {
        current_state.status = new_state.status;
        tx.send(PlayerEvent::StatusChanged(new_state.status.clone())).await?;
    }
    Ok(())
}

async fn update_timeline(new_state: &PlayerState,
                         current_state: &mut PlayerState,
                         tx: &mut Sender<PlayerEvent>) -> Result<(), SendError> {
    if new_state.timeline != current_state.timeline {
        current_state.timeline = new_state.timeline.clone();
        tx.send(PlayerEvent::TimelineChanged(new_state.timeline.clone())).await?;
    }
    Ok(())
}

async fn update_text(text_id: FsctTextMetadata,
                     new_state: &PlayerState,
                     current_state: &mut PlayerState,
                     tx: &mut Sender<PlayerEvent>) -> Result<(), SendError>
{
    let new_text = new_state.texts.get_text(text_id);
    let current_text = current_state.texts.get_mut_text(text_id);
    if new_text != current_text {
        *current_text = new_text.clone();
        tx.send(PlayerEvent::TextChanged((text_id, new_text.clone()))).await?;
    }
    Ok(())
}
async fn update_texts(new_state: &PlayerState,
                      current_state: &mut PlayerState,
                      tx: &mut Sender<PlayerEvent>) -> Result<(), SendError> {
    update_text(FsctTextMetadata::CurrentTitle, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentAuthor, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentAlbum, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentGenre, new_state, current_state, tx).await?;
    update_text(FsctTextMetadata::CurrentYear, new_state, current_state, tx).await?;

    Ok(())
}

async fn update_current_metadata(new_state: &PlayerState,
                                 current_state: &mut PlayerState,
                                 tx: &mut Sender<PlayerEvent>) -> Result<(), SendError>
{
    update_current_status(new_state, current_state, tx).await?;
    update_timeline(new_state, current_state, tx).await?;
    update_texts(new_state, current_state, tx).await?;
    Ok(())
}


fn create_polling_metadata_watch(player: Player) -> PlayerEventListener
{
    let (mut tx, rx) = futures::channel::mpsc::channel(30);
    tokio::spawn(async move {
        let mut current_metadata = PlayerState::default();
        loop {
            let state = player.get_current_state().await.inspect_err(|e|
                error!("Failed to get current state: {}", e)
            ).unwrap_or_default();

            if let Err(e) = update_current_metadata(&state, &mut current_metadata, &mut tx).await {
                if e.is_disconnected() {
                    break;
                }
                error!("Failed to send changes to channel: {}", e);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    rx
}

fn update_current_state_on_event(event: &PlayerEvent, current_state: &mut PlayerState) -> bool {
    match event {
        PlayerEvent::StatusChanged(status) => {
            if *status != current_state.status {
                current_state.status = status.clone();
                println!("Status changed to {:?}", current_state.status);
                return true;
            }
        }
        PlayerEvent::TimelineChanged(timeline) => {
            if *timeline != current_state.timeline {
                current_state.timeline = timeline.clone();
                println!("Timeline changed to {:?}", current_state.timeline);
                return true;
            }
        }
        PlayerEvent::TextChanged((text_id, text)) => {
            let current_text = current_state.texts.get_mut_text(*text_id);
            if *text != *current_text {
                *current_text = text.clone();
                println!("Text {:?} changed to {:?}", text_id, text);
                return true;
            }
        }
    };
    false
}
async fn process_player_event(event: PlayerEvent, fsct_devices: &DeviceMap, current_metadata:
&Arc<Mutex<PlayerState>>)
{
    let has_changed = update_current_state_on_event(&event, &mut current_metadata.lock().unwrap());
    if !has_changed {
        return;
    }

    apply_event_on_devices(fsct_devices, event).await;
}

async fn apply_event_on_devices(fsct_devices: &DeviceMap, event: PlayerEvent) {
    let devices = fsct_devices.lock().unwrap().values().cloned().collect::<Vec<_>>();
    for device in devices {
        let result = apply_event_on_device(&device, &event).await;
        if let Err(e) = result {
            error!("Failed to apply changes on device: {}", e);
        }
    }
}

async fn apply_event_on_device(fsct_device: &FsctDevice, event: &PlayerEvent) -> Result<(), String> {
    match event {
        PlayerEvent::StatusChanged(status) => fsct_device.set_status(status.clone()).await?,
        PlayerEvent::TimelineChanged(timeline) => fsct_device.set_progress(timeline.clone()).await?,
        PlayerEvent::TextChanged((text_id, text)) => fsct_device.set_current_text(text_id.clone(), text.as_ref().map(|s| s.as_str())).await?
    }
    Ok(())
}

async fn get_playback_notification_stream(player: Player) -> Result<PlayerEventListener, PlayerError>
{
    match player.listen_to_player_notifications().await {
        Ok(listener) => Ok(listener),
        Err(PlayerError::FeatureNotSupported) => Ok(create_polling_metadata_watch(player)),
        Err(e) => Err(e),
    }
}

async fn run_player_watch(fsct_devices: DeviceMap,
                          player: Player,
                          current_metadata: Arc<Mutex<PlayerState>>)
    -> Result<(), String>
{
    let mut playback_notifications_stream = get_playback_notification_stream(player).await.map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        while let Some(event) = playback_notifications_stream.next().await {
            process_player_event(event, &fsct_devices, &current_metadata).await;
        }
    });
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

pub async fn run_service(player: Player) -> Result<(), String> {
    let fsct_devices = Arc::new(Mutex::new(HashMap::new()));
    let current_metadata = Arc::new(Mutex::new(PlayerState::default()));
    run_devices_watch(fsct_devices.clone(), current_metadata.clone()).await?;
    run_player_watch(fsct_devices.clone(), player, current_metadata).await?;

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C signal");
    println!("Exiting...");
    Ok(())
}