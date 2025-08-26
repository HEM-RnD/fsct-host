
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

    // Connect client (this performs handshake and verifies protocol version)
    let client = IpcDriver::connect_to_endpoint(endpoint).await?;
    let ver = client.get_protocol_version().await?;

    println!("Client received protocol version: {}.{}", ver.major, ver.minor);
    assert_eq!(ver, FSCT_PROTOCOL_VERSION);

    Ok(())
}
