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

use crate::initialize_native_platform_player;
use anyhow::anyhow;
use env_logger::Env;
use fsct_core::run_service;

#[tokio::main(flavor = "current_thread")]
pub async fn fsct_main() -> anyhow::Result<()> {
    let env = Env::default()
        .filter_or("FSCT_LOG", "info")
        .write_style("FSCT_LOG_STYLE");
    env_logger::init_from_env(env);

    let platform_global_player = initialize_native_platform_player().await.map_err(|e| anyhow!(e))?;
    let devices_watch_handle = run_service(platform_global_player).await?;

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C signal");
    println!("Stopping service.");
    let res = devices_watch_handle.shutdown().await;
    if let Err(e) = res {
        println!("Error while stopping service: {}", e);
        return Err(e.into());
    }
    println!("Exit.");
    Ok(())
}
