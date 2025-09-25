use std::sync::Arc;
use fsct_core::{FsctDriver, LocalDriver, MultiServiceHandle};
use log::{info, warn};
use anyhow::anyhow;
use crate::cli::{Cli, Parser};
use crate::socket_path;
use crate::run_os_watcher;

pub trait StopSignal {
    async fn wait(&self) -> anyhow::Result<()>;
}

pub async fn async_main(stop_signal: impl StopSignal) -> anyhow::Result<()> {
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

    let mut clean_socket = false;
    let mut ok = true;

    if args.driver {
        // In driver mode, expose IPC driver over IPC and do not start OS watcher
        let ipc = if let Some(fd) = socket_activation_fd {
            if args.endpoint.is_some() {
                warn!("Ignoring --socket argument because systemd socket activation is detected");
            }
            fsct_core::ipc::server::run_ipc_server_with_fd(driver.clone(), fd)
        } else {
            // remove potential stale socket, ignore if not present
            std::fs::remove_file(endpoint.as_str()).ok();
            // set socket to be removed in the end of the function
            clean_socket = true;
            fsct_core::ipc::server::run_ipc_server_with_endpoint_path(driver.clone(), endpoint.clone())
        };
        services.add(ipc);
    } else {
        // In user and standalone modes, start OS watcher and connect to driver (IPC or in-process)
        if let Ok(watcher) = run_os_watcher(driver.clone()).await {
            services.add(watcher);
        } else {
            warn!("Failed to start OS watcher");
            ok = false;
        }
    }

    // wait for finish only if everything started successfully, otherwise jump directly to shutdown
    let service_res = if ok {
        tokio::select! {
            _ = stop_signal.wait() => {Ok(())},
            res = services.wait_for_any_to_finish() => {
                res.inspect_err(|e| log::error!("Service error: {}", e))
            },
        }
    } else {
        Ok(())
    };

    let shutdown_res = services.shutdown().await
                               .inspect_err(|e| log::error!("Shutdown error: {}", e));

    if clean_socket == false {
        if let Err(r) = std::fs::remove_file(endpoint) {
            log::warn!("Failed to remove IPC socket file: {}", r);
        }
    }

    service_res?;
    shutdown_res?;
    Ok(())
}