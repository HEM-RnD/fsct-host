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

use std::collections::HashMap;
use async_trait::async_trait;
use fsct_core::definitions::{FsctStatus, FsctTextMetadata};
use fsct_core::player::{
    create_player_events_channel, PlayerError, PlayerEvent, PlayerEventsReceiver,
    PlayerEventsSender,
};
use fsct_core::{player::Player, player::PlayerInterface, player::PlayerState, run_devices_watch, run_player_watch, DevicesPlayerEventApplier, DevicesWatchHandle};
use std::sync::{Arc, Mutex};
use tokio::task::{AbortHandle};
use js_types::{CurrentTextMetadata, FsctTimelineInfo, PlayerStatus, TimelineInfo};

pub struct NodePlayerImpl {
    current_state: Mutex<PlayerState>,
    player_sender: PlayerEventsSender,
}

impl NodePlayerImpl {
    fn new() -> Self {
        let (tx, _rx) = create_player_events_channel();
        Self {
            current_state: Mutex::new(PlayerState::default()),
            player_sender: tx,
        }
    }

    fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        let status: FsctStatus = status.into();
        self.current_state.lock().unwrap().status = status;

        self.emit(PlayerEvent::StatusChanged(status))
    }

    fn set_timeline(&self, timeline: Option<TimelineInfo>) -> napi::Result<()> {
        let timeline: Option<FsctTimelineInfo> = timeline.map(|v| v.try_into().ok()).flatten();
        self.current_state.lock().unwrap().timeline = timeline.clone();

        self.emit(PlayerEvent::TimelineChanged(timeline))
    }

    fn set_text(&self, text_type: CurrentTextMetadata, text: Option<String>) -> napi::Result<()> {
        let text_type: FsctTextMetadata = text_type.into();
        *self
            .current_state
            .lock()
            .unwrap()
            .texts
            .get_mut_text(text_type) = text.clone();

        self.emit(PlayerEvent::TextChanged((text_type, text.clone())))
    }

    fn emit(&self, event: PlayerEvent) -> napi::Result<()> {
        self.player_sender
            .send(event)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl PlayerInterface for NodePlayerImpl {
    async fn get_current_state(&self) -> Result<PlayerState, PlayerError> {
        Ok(self.current_state.lock().unwrap().clone())
    }

    async fn listen_to_player_notifications(&self) -> Result<PlayerEventsReceiver, PlayerError> {
        Ok(self.player_sender.subscribe())
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
    pub fn set_status(&self, status: PlayerStatus) -> napi::Result<()> {
        self.player_impl.set_status(status)
    }

    #[napi]
    pub fn set_timeline(&self, timeline: Option<TimelineInfo>) -> napi::Result<()> {
        self.player_impl.set_timeline(timeline)
    }

    #[napi]
    pub fn set_text(
        &self,
        text_type: CurrentTextMetadata,
        text: Option<String>,
    ) -> napi::Result<()> {
        self.player_impl.set_text(text_type, text)
    }
}

struct FsctServiceAbortHandle {
    device_watch_handle: DevicesWatchHandle,
    player_watch_handle: AbortHandle,
}

impl FsctServiceAbortHandle {
    // Synchronous abort for use in Drop and other non-async contexts
    async fn stop(self) {
        // Abort player watch task immediately
 

        // Spawn a task to shutdown device watch handle
        // This allows abort to be synchronous while still properly shutting down the device watch
        // tokio::spawn(async move {
        //     if let Err(e) = self.device_watch_handle.shutdown().await {
        //         log::error!("Error shutting down device watch: {}", e);
        //     }
        //     // Wait a short time to allow tasks to clean up
        //     tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        // });

        if let Err(e) = self.device_watch_handle.shutdown().await {
            log::error!("Error shutting down device watch: {}", e);
            //todo handle error 
        }
        self.player_watch_handle.abort();
    }
    
    fn abort(self) {
        self.device_watch_handle.abort();
        self.player_watch_handle.abort();
    }
}

async fn run_fsct(player: &NodePlayer) -> napi::Result<FsctServiceAbortHandle> {
    let fsct_devices = Arc::new(Mutex::new(HashMap::new()));
    let player_state = Arc::new(Mutex::new(PlayerState::default()));

    let player_event_listener = DevicesPlayerEventApplier::new(fsct_devices.clone());

    let device_watch_handle = run_devices_watch(fsct_devices.clone(), player_state.clone()).await.map_err(|e|
        napi::Error::from_reason(e.to_string()))?;
    let player_watch_handle = run_player_watch(Player::from_arc(player.player_impl.clone()), player_event_listener,
                                               player_state).await
                                                            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(FsctServiceAbortHandle {
        device_watch_handle,
        player_watch_handle: player_watch_handle.abort_handle(),
    })
}


#[napi]
pub struct FsctService {
    service_abort_handle: Mutex<Option<FsctServiceAbortHandle>>,
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
            service_abort_handle: Mutex::new(None)
        }
    }

    #[napi]
    pub async fn run_fsct(&self, player: &NodePlayer) -> napi::Result<()> {
        if self.service_abort_handle.lock().unwrap().is_some() {
            // if we know at this point that service is already run we don't even try to start it
            return Err(napi::Error::from_reason("FSCT service already run"));
        }
        let new_service_abort_handle = run_fsct(player).await?;
        {
            let mut service_impl = self.service_abort_handle.lock().unwrap();
            if service_impl.is_none() {
                service_impl.replace(new_service_abort_handle);
                return Ok(());
            }
        }
        
        // if for some reason service has started, during we are starting our new service (i.e. typical race
        // condition), we abort the new service.
        new_service_abort_handle.stop().await;
        return Err(napi::Error::from_reason("FSCT service already run"));
    }

    #[napi]
    pub async fn stop_fsct(&self) -> napi::Result<()> {
        let abort_handle = self
            .service_abort_handle.lock().unwrap()
            .take()
            .ok_or_else(|| napi::Error::from_reason("FSCT service not run"))?;
        abort_handle.stop().await;
        Ok(())
    }
}

#[napi]
impl Drop for FsctService {
    fn drop(&mut self) {
        let abort_handle = self
            .service_abort_handle.lock().unwrap()
            .take();

        if let Some(abort_handle) = abort_handle {
            abort_handle.abort();
        }
    }
}
