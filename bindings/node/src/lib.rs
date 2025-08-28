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

#![deny(clippy::all)]

mod js_types;

#[macro_use]
extern crate napi_derive;

use fsct_core::definitions::{FsctStatus, FsctTextMetadata};
use fsct_core::player_state::PlayerState;
use fsct_core::{FsctDriver, LocalDriver, ManagedPlayerId, service::MultiServiceHandle};
use std::sync::{Arc, Mutex};
use js_types::{CurrentTextMetadata, FsctTimelineInfo, PlayerStatus, TimelineInfo};

pub struct NodePlayerImpl {
    current_state: Mutex<PlayerState>,
    driver: Mutex<Option<Arc<LocalDriver>>>,
    player_id: Mutex<Option<ManagedPlayerId>>,
}

impl NodePlayerImpl {
    fn new() -> Self {
        Self {
            current_state: Mutex::new(PlayerState::default()),
            driver: Mutex::new(None),
            player_id: Mutex::new(None),
        }
    }

    async fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        let status: FsctStatus = status.into();
        self.current_state.lock().unwrap().status = status;
        self.push_state().await
    }

    async fn set_timeline(&self, timeline: Option<TimelineInfo>) -> napi::Result<()> {
        let timeline: Option<FsctTimelineInfo> = timeline.and_then(|v| v.try_into().ok());
        self.current_state.lock().unwrap().timeline = timeline;
        self.push_state().await
    }

    async fn set_text(&self, text_type: CurrentTextMetadata, text: Option<String>) -> napi::Result<()> {
        let text_type: FsctTextMetadata = text_type.into();
        *self
            .current_state
            .lock()
            .unwrap()
            .texts
            .get_mut_text(text_type) = text;
        self.push_state().await
    }

    async fn push_state(&self) -> napi::Result<()> {
        let state = self.current_state.lock().unwrap().clone();
        let driver_opt = self.driver.lock().unwrap().clone();
        let player_id_opt = *self.player_id.lock().unwrap();
        if let (Some(driver), Some(player_id)) = (driver_opt, player_id_opt) {
            driver
                .update_player_state(player_id, state)
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?
        }
        Ok(())
    }

    async fn attach_driver_and_register(&self, driver: Arc<LocalDriver>, self_id: String) -> napi::Result<()> {
        let player_id = driver
            .register_player(self_id)
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        *self.driver.lock().unwrap() = Some(driver);
        *self.player_id.lock().unwrap() = Some(player_id);
        // push initial default state
        self.push_state().await
    }
}

#[napi]
pub struct NodePlayer {
    player_impl: Arc<NodePlayerImpl>,
}

#[napi]
impl NodePlayer {
    #[napi(constructor)]
    pub fn new() -> Self {
        NodePlayer {
            player_impl: Arc::new(NodePlayerImpl::new()),
        }
    }

    #[napi]
    pub async fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        self.player_impl.set_status(status).await
    }

    #[napi]
    pub async fn set_timeline(&self, timeline: Option<TimelineInfo>) -> napi::Result<()> {
        self.player_impl.set_timeline(timeline).await
    }

    #[napi]
    pub async fn set_text(
        &self,
        text_type: CurrentTextMetadata,
        text: Option<String>,
    ) -> napi::Result<()> {
        self.player_impl.set_text(text_type, text).await
    }
}


#[napi]
pub struct FsctService {
    driver: Mutex<Option<Arc<LocalDriver>>>,
    service_handle: Mutex<Option<MultiServiceHandle>>,
}

#[napi]
pub enum LogLevelFilter {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Off,
}

impl From<LogLevelFilter> for log::LevelFilter {
    fn from(level: LogLevelFilter) -> Self {
        match level {
            LogLevelFilter::Trace => log::LevelFilter::Trace,
            LogLevelFilter::Debug => log::LevelFilter::Debug,
            LogLevelFilter::Info => log::LevelFilter::Info,
            LogLevelFilter::Warn => log::LevelFilter::Warn,
            LogLevelFilter::Error => log::LevelFilter::Error,
            LogLevelFilter::Off => log::LevelFilter::Off,
        }
    }
}

#[napi]
pub fn init_stdout_logger() -> Result<(), napi::Error> {
    env_logger::init();
    Ok(())
}

#[allow(unreachable_code, unused_variables)]
#[napi]
pub fn init_systemd_logger(syslog_identifier: String) -> Result<(), napi::Error> {
    #[cfg(target_os = "linux")]
    {
        use systemd_journal_logger::JournalLog;

        return JournalLog::new()?
            .with_syslog_identifier(syslog_identifier)
            .install().map_err(|e| napi::Error::from_reason(e.to_string()));
    }

    Err(napi::Error::from_reason("systemd logger not supported on this platform"))
}


#[napi]
pub fn set_log_level(level: LogLevelFilter) {
    log::set_max_level(level.into());
}

#[napi]
impl FsctService {
    #[napi(constructor)]
    pub fn new() -> Self {
        FsctService {
            driver: Mutex::new(None),
            service_handle: Mutex::new(None),
        }
    }

    #[napi]
    pub async fn run_fsct(&self, player: &NodePlayer) -> napi::Result<()> {
        if self.service_handle.lock().unwrap().is_some() {
            return Err(napi::Error::from_reason("FSCT service already run"));
        }

        // Create driver and run background services
        let driver = Arc::new(LocalDriver::with_new_managers());
        let handle = driver
            .run()
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        // Register the node player with the driver and attach it
        player
            .player_impl
            .attach_driver_and_register(driver.clone(), "node-js".to_string())
            .await?;

        // Store driver and handle if still empty (avoid race)
        {
            let mut guard = self.service_handle.lock().unwrap();
            if guard.is_none() {
                *self.driver.lock().unwrap() = Some(driver);
                *guard = Some(handle);
                return Ok(());
            }
        }

        // If another runner won the race, shutdown the newly created handle and return error
        handle
            .shutdown()
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Err(napi::Error::from_reason("FSCT service already run"))
    }

    #[napi]
    pub async fn stop_fsct(&self) -> napi::Result<()> {
        // Take handle and driver
        let handle = self
            .service_handle
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| napi::Error::from_reason("FSCT service not run"))?;
        *self.driver.lock().unwrap() = None;

        handle
            .shutdown()
            .await
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }
}

#[napi]
impl Drop for FsctService {
    fn drop(&mut self) {
        // Just drop the handle and driver; we cannot async shutdown here
        let _ = self.service_handle.lock().unwrap().take();
        let _ = self.driver.lock().unwrap().take();
    }
}
