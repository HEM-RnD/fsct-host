// Minimal example of using the unified FsctDriver trait with LocalDriver
use std::time::Duration;

use anyhow::Result;
use fsct_core::{FsctDriver, LocalDriver, PlayerState};
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use log::info;
use fsct_core::player_state::TrackMetadata;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // Create local in-process driver
    let driver = LocalDriver::with_new_managers();

    // Start orchestrator and USB device watch services
    let handle = driver.run().await?;

    // Subscribe to events (optional in this example)
    let mut _player_rx = driver.subscribe_player_events();

    // Register a player and update its state
    let player_id = driver.register_player("driver-example".to_string()).await?;

    let state = PlayerState {
        status: FsctStatus::Playing,
        timeline: Some(TimelineInfo {
            position: Duration::from_secs(5),
            duration: Duration::from_secs(200),
            rate: 1.0,
            update_time: std::time::SystemTime::now(),
        }),
        texts: TrackMetadata {
            title: Option::from("Пісня Сміливих Дівчат".to_string()),
            artist: Option::from("KAZKA".to_string()),
            ..Default::default()
        }
    };

    driver.update_player_state(player_id, state).await?;

    // Preferred player can be set via driver as well
    driver.set_preferred_player(Some(player_id))?;

    info!("Driver example is running. Press Ctrl+C to shut down.");

    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
    info!("Ctrl+C received, shutting down services...");

    // Gracefully shutdown background services
    handle.shutdown().await.expect("failed to shutdown services");
    info!("Services shut down. Exiting.");

    Ok(())
}
