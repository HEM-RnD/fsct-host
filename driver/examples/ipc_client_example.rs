use fsct::definitions::{FsctStatus, TimelineInfo};
use fsct::player_state::TrackMetadata;
use fsct::{FSCT_PROTOCOL_VERSION, FsctDriver, PlayerState};
use fsct_client::IpcDriver;
use log::info;
use std::time::Duration;

async fn print_device_info(driver: &IpcDriver, device_id: uuid::Uuid) {
    let device_info = driver.get_device_info(device_id).await.unwrap();
    info!("Device info: {:?}", device_info);
}

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
        },
    };

    driver.update_player_state(player_id, state).await?;

    let connected_devices = driver.get_detected_devices().await?;
    for device in connected_devices {
        print_device_info(&driver, device).await;
    }

    let mut rx = driver.subscribe_device_changes().await?;
    let handle = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                fsct::DeviceChangeEvent::Added(uuid) => {
                    info!("Device added: {}", uuid);
                    print_device_info(&driver, uuid).await;
                }
                fsct::DeviceChangeEvent::Removed(uuid) => {
                    info!("Device removed: {}", uuid);
                }
            }
        }
    });

    info!("Driver example is running. Press Ctrl+C to shut down.");

    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
    info!("Ctrl+C received, shutting down services...");

    handle.abort();
    Ok(())
}
