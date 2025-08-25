// Example: standalone IPC server exposing FsctDriver over msgpack-rpc
use std::sync::Arc;
use fsct_core::ipc::server::IpcServer;
use fsct_core::LocalDriver;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    #[cfg(windows)]
    let endpoint = "\\\\.\\pipe\\fsct_host_example".to_string();
    #[cfg(unix)]
    let endpoint = "/tmp/fsct_host_example.sock".to_string();

    let driver = Arc::new(LocalDriver::with_new_managers());
    let handle = driver.run().await?;

    let server = IpcServer::with_endpoint(driver, endpoint.clone());

    println!("FSCT IPC server example listening on: {endpoint}");
    println!("Press Ctrl+C to stop...");

    // Run the server and concurrently wait for Ctrl+C to shut down
    tokio::select! {
        res = server.serve() => {
            if let Err(e) = res {
                eprintln!("Server terminated with error: {e}");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("Shutdown signal received. Exiting...");
        }
    }

    handle.shutdown().await?;

    Ok(())
}
