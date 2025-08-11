// Example showing how to wire PlayerManager + DeviceManager + UsbDeviceWatch + Orchestrator
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use fsct_core::{
    DeviceManager, run_usb_device_watch, Orchestrator, PlayerManager,
};
use fsct_core::PlayerState;
use log::info;
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::player::TrackMetadata;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let player_manager = PlayerManager::new();
    let player_events = player_manager.subscribe();

    let device_manager = Arc::new(DeviceManager::new());
    let _usb_watch = run_usb_device_watch(device_manager.clone()).await?;

    // Start orchestrator
    let orchestrator = Orchestrator::with_device_manager(player_events, device_manager.clone());
    let _orch_handle = orchestrator.run();

    // Demo: create a player and update some state
    let player_id = player_manager.register_player("demo-player".to_string()).await?;

    let mut state = PlayerState {
         status: FsctStatus::Playing,
         timeline: Some(TimelineInfo{
             position: Duration::from_secs(13),
             duration: Duration::from_secs(184),
             rate: 1.0,
             update_time: std::time::SystemTime::now()
         }),
        texts: TrackMetadata {
            artist: Some("Demo Artist".to_string()),
            title: Some("Demo title".to_string()),
            ..Default::default()
        },
    };
    // do some small changes if needed; for now defaults
    player_manager.update_player_state(player_id, state.clone()).await?;

    info!("Orchestrator example running; press Ctrl+C to exit");
    tokio::signal::ctrl_c().await?;
    let orch_shutdown_task = _orch_handle.shutdown();
    let usb_watch_shutdown_task = _usb_watch.shutdown();
    let (r1, r2) = tokio::join!(orch_shutdown_task, usb_watch_shutdown_task);
    r1?;
    r2?;
    Ok(())
}
