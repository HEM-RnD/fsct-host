use std::sync::Arc;
use std::time::Duration;
use fsct_core::{FsctDriver, LocalDriver, MultiServiceHandle};
use log::{debug, error, info, warn};
use anyhow::anyhow;
use crate::cli::{Cli, Parser};
use crate::socket_path;
use crate::run_os_watcher;

pub trait StopSignal {
    fn wait(&mut self) -> impl Future<Output = anyhow::Result<()>> + Send;
}

pub trait ServiceStateListener {
    fn on_service_started(&self) -> anyhow::Result<()>;
    fn on_service_stopping(&self) -> anyhow::Result<()>;
}

pub struct ServiceStateNullListener;

impl ServiceStateListener for ServiceStateNullListener {
    fn on_service_started(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn on_service_stopping(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

pub async fn async_main(mut stop_signal: impl StopSignal, listener: impl ServiceStateListener) -> anyhow::Result<()> {
    // Parse CLI
    let args = Cli::parse();

    let mut services = MultiServiceHandle::new();

    let endpoint = args.endpoint.clone().unwrap_or_else(|| socket_path().to_string());

    let driver: Arc<dyn FsctDriver> = if args.user {
        // In user mode, connect to IPC driver
        info!("Connecting to IPC driver at {}", endpoint);
        Arc::new(fsct_core::ipc::client::IpcDriver::connect_to_endpoint(endpoint.clone()).await?)
    } else {
        // in driver and standalone mode use in-process driver
        let driver = Arc::new(LocalDriver::with_new_managers());
        services = driver.run().await.map_err(|e| anyhow!(e))?;
        driver
    };

    let socket_activation_fd = crate::get_socket_activation_fd();

    let success = if args.driver {
        // In driver mode, expose IPC driver over IPC and do not start OS watcher
        let ipc = if let Some(fd) = socket_activation_fd {
            if args.endpoint.is_some() {
                warn!("Ignoring --socket argument because systemd socket activation is detected");
            }
            fsct_core::ipc::server::run_ipc_server_with_fd(driver.clone(), fd)
        } else {
            fsct_core::ipc::server::run_ipc_server_with_endpoint_path(driver.clone(), endpoint.clone())
        };
        services.add(ipc);
        true
    } else {
        // In user and standalone modes, start OS watcher and connect to driver (IPC or in-process)
        debug!("Initializing native platform player");
        let mut retries = 0;
        loop {
            match run_os_watcher(driver.clone()).await {
                Ok(player) => {
                    services.add(player);
                    break true;
                }
                Err(e) => {
                    retries += 1;
                    if retries >= 10 {
                        error!("Failed to initialize player after 10 retries: {:?}", e);
                        break false;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    debug!("Retrying initialization, attempt {}/10", retries + 1);
                }
            }
        }
    };

    // wait for finish only if everything started successfully, otherwise jump directly to shutdown
    let service_res = if success {
        if let Err(e) = listener.on_service_started() {
            error!("Error during notifying service started: {:?}", e);
            Err(anyhow!(e))
        } else {
            tokio::select! {
                _ = stop_signal.wait() => {Ok(())},
                res = services.wait_for_any_to_finish() => {
                    res.inspect_err(|e| log::error!("Service error: {}", e))
                },
            }.map_err(|e|anyhow!(e))
        }
    } else {
        Ok(())
    };
    if let Err(e) = listener.on_service_stopping() {
        error!("Failed to notify service stopping: {}", e);
    }

    let shutdown_res = services.shutdown().await
                               .inspect_err(|e| log::error!("Shutdown error: {}", e));

    service_res?;
    shutdown_res?;
    Ok(())
}