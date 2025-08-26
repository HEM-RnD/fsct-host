// Integration test for IPC server<->client fetching protocol version
use std::sync::Arc;
use std::time::Duration;

use fsct_core::{FSCT_PROTOCOL_VERSION, LocalDriver};
use fsct_core::ipc::server::IpcServer;
use fsct_core::ipc::client::IpcDriver;

fn test_endpoint() -> String {
    #[cfg(windows)]
    {
        // Unique Windows named pipe per test run
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
async fn ipc_get_protocol_version() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let endpoint = test_endpoint();

    // Start server with a simple local driver (no background services needed for this test)
    let driver = Arc::new(LocalDriver::with_new_managers());
    let server = IpcServer::with_endpoint(driver, endpoint.clone());

    let server_task = tokio::spawn(async move {
        // If the server ends with an error, just return it; the test will abort anyway
        let _ = server.serve().await;
    });

    // Retry connecting the client until the server is listening (with timeout)
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);

    let client = loop {
        match IpcDriver::connect_to_endpoint(endpoint.clone()).await {
            Ok(c) => break c,
            Err(e) => {
                if start.elapsed() > timeout {
                    server_task.abort();
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    };

    // Verify version
    let ver = client.get_protocol_version().await?;
    assert_eq!(ver, FSCT_PROTOCOL_VERSION);

    // Stop server
    server_task.abort();

    // Clean up leftover unix domain socket file if any
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(endpoint);
    }

    Ok(())
}
