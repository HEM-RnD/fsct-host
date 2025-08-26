use std::time::Duration;
use log::info;
use fsct_core::{FsctDriver, PlayerState, FSCT_PROTOCOL_VERSION};
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::ipc::client::IpcDriver;
use fsct_core::player_state::TrackMetadata;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Choose a unique endpoint for the example to avoid conflicts
    #[cfg(windows)]
    let endpoint = "\\\\.\\pipe\\fsct_host_example".to_string();
    #[cfg(unix)]
    let endpoint = "/tmp/fsct_host_example.sock".to_string();

    // Connect client (this performs handshake and verifies protocol version)
    let driver = IpcDriver::connect_to_endpoint(endpoint).await?;
    let ver = driver.get_protocol_version().await?;

    println!("Client received protocol version: {}.{}", ver.major, ver.minor);
    assert_eq!(ver, FSCT_PROTOCOL_VERSION);

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
    driver.set_preferred_player(Some(player_id)).await?;

    info!("Driver example is running. Press Ctrl+C to shut down.");

    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
    info!("Ctrl+C received, shutting down services...");

    Ok(())
}
