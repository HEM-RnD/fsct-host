// Integration test for IPC server<->client register/unregister player
use std::sync::Arc;
use std::time::Duration;

use fsct_core::LocalDriver;
use fsct_core::ipc::server::IpcServer;
use fsct_core::ipc::client::IpcDriver;
use fsct_core::FsctDriver;

fn test_endpoint() -> String {
    #[cfg(windows)]
    {
        let suffix = format!("{}", uuid::Uuid::new_v4());
        return format!("\\\\.\\pipe\\fsct_host_test_{}", suffix);
    }
    #[cfg(unix)]
    {
        let suffix = format!("{}", uuid::Uuid::new_v4());
        let mut path = std::env::temp_dir();
        path.push(format!("fsct_host_test_{}.sock", suffix));
        return path.to_string_lossy().to_string();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipc_register_and_unregister_player() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let endpoint = test_endpoint();

    // Start server with LocalDriver
    let driver = Arc::new(LocalDriver::with_new_managers());
    let server = IpcServer::with_endpoint(driver, endpoint.clone());

    let server_task = tokio::spawn(async move {
        let _ = server.serve().await;
    });

    // Retry connect loop until server ready
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);
    let client = loop {
        match IpcDriver::connect_to_endpoint(endpoint.clone()).await {
            Ok(c) => break c,
            Err(_) => {
                if start.elapsed() > timeout {
                    server_task.abort();
                    panic!("Failed to connect to IPC server in time");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };

    // Register a player
    let player_id = client.register_player("test_player_self_id".to_string()).await?;
    assert!(player_id.get() > 0);

    // Unregister the player
    client.unregister_player(player_id).await?;

    // Stop server
    server_task.abort();

    // Clean up leftover unix domain socket file if any
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(endpoint);
    }

    Ok(())
}
