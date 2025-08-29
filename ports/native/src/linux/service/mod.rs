// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

use anyhow::anyhow;
use env_logger::Env;
use fsct_core::LocalDriver;
use std::sync::Arc;
use crate::run_os_watcher;

/// Linux service entrypoint (skeleton implementation).
///
/// It initializes logging, starts the LocalDriver (orchestrator + USB watch),
/// starts a placeholder OS watcher (no-op for now), and waits for Ctrl+C for a graceful shutdown.
#[tokio::main(flavor = "current_thread")]
pub async fn fsct_main() -> anyhow::Result<()> {
    let env = Env::default()
        .filter_or("FSCT_LOG", "info")
        .write_style("FSCT_LOG_STYLE");
    env_logger::init_from_env(env);

    // Initialize local driver and run background services (orchestrator + USB watch)
    let driver = Arc::new(LocalDriver::with_new_managers());
    let mut handle = driver.run().await.map_err(|e| anyhow!(e))?;

    // Start Linux OS watcher placeholder (to be replaced with real MPRIS/DBus integration)
    if let Ok(watcher) = run_os_watcher(driver.clone()).await {
        handle.add(watcher);
    }

    // Wait for termination signal
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C signal");
    println!("Stopping service.");

    let res = handle.shutdown().await;
    if let Err(e) = res {
        println!("Error while stopping service: {}", e);
        return Err(e.into());
    }
    println!("Exit.");
    Ok(())
}
