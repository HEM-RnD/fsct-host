// Example: standalone IPC server exposing FsctDriver over msgpack-rpc
use std::sync::Arc;
use fsct::LocalDriver;
use fsct_driver::ipc::run_ipc_server_with_endpoint_path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    #[cfg(windows)]
    let endpoint = "\\\\.\\pipe\\fsct_host_example".to_string();
    #[cfg(unix)]
    let endpoint = "/tmp/fsct_host_example.sock".to_string();

    let driver = Arc::new(LocalDriver::with_new_managers());
    let handle = driver.run().await?;

    let ipc_handle = run_ipc_server_with_endpoint_path(driver.clone(), endpoint.clone());

    println!("FSCT IPC server example listening on: {endpoint}");
    println!("Press Ctrl+C to stop...");

    // Wait for Ctrl+C then gracefully shutdown services
    tokio::signal::ctrl_c().await?;
    println!("Shutdown signal received. Exiting...");

    ipc_handle.shutdown().await?;
    handle.shutdown().await?;

    Ok(())
}
