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
use crate::async_main;

pub struct UnixStopSignal {}
impl UnixStopSignal {
    fn new() -> Self {
        Self {}
    }
}
impl async_main::StopSignal for UnixStopSignal{
    async fn wait(&self) -> anyhow::Result<()> {
        let mut terminate_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            res = tokio::signal::ctrl_c() => res.map_err(|e| e.into()),
            res = terminate_signal.recv() => res.ok_or_else(|| anyhow!("Terminate signal receiving failed")),
        }
    }
}

/// Linux service entrypoint with CLI to choose mode (standalone/driver/user).
#[tokio::main(flavor = "current_thread")]
pub async fn fsct_main() -> anyhow::Result<()> {
    // Initialize logging via env_logger (FSCT_LOG, FSCT_LOG_STYLE)
    let env = Env::default()
        .filter_or("FSCT_LOG", "info")
        .write_style("FSCT_LOG_STYLE");
    env_logger::init_from_env(env);

    let stop_signal = UnixStopSignal::new();
    async_main::async_main(stop_signal).await
}

