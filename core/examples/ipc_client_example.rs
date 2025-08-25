
use fsct_core::{FSCT_PROTOCOL_VERSION};
use fsct_core::ipc::client::IpcDriver;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Choose a unique endpoint for the example to avoid conflicts
    #[cfg(windows)]
    let endpoint = "\\\\.\\pipe\\fsct_host_example".to_string();
    #[cfg(unix)]
    let endpoint = "/tmp/fsct_host_example.sock".to_string();

    // Create client and call get_protocol_version
    let client = IpcDriver::with_endpoint(endpoint);
    let ver = client.get_protocol_version().await?;

    println!("Client received protocol version: {}.{}", ver.major, ver.minor);
    assert_eq!(ver, FSCT_PROTOCOL_VERSION);

    Ok(())
}
