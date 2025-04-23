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
use fsct_core::{player::Player, player::PlayerInterface, player::PlayerState, run_devices_watch, run_player_watch, DevicesPlayerEventApplier};
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
        let timeline: Option<FsctTimelineInfo> = timeline.map(|v| v.into());
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
    device_watch_handle: AbortHandle,
    player_watch_handle: AbortHandle,
}

impl FsctServiceAbortHandle {
    fn abort(&self) {
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
        device_watch_handle: device_watch_handle.abort_handle(),
        player_watch_handle: player_watch_handle.abort_handle(),
    })
}


#[napi]
pub struct FsctService {
    service_abort_handle: Mutex<Option<FsctServiceAbortHandle>>,
}

#[napi]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for log::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => log::Level::Trace,
            LogLevel::Debug => log::Level::Debug,
            LogLevel::Info => log::Level::Info,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Error => log::Level::Error,
        }
    }
}
#[napi]
pub fn init_logger(level: LogLevel) -> Result<(), napi::Error>
{
    simple_logger::init_with_level(level.into()).map_err(|e| napi::Error::from_reason(e.to_string()))
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
        let mut service_impl = self.service_abort_handle.lock().unwrap();
        if service_impl.is_some() {
            // if for some reason service has started, during we are starting our new service (i.e. typical race
            // condition), we abort the new service.
            new_service_abort_handle.abort();
            return Err(napi::Error::from_reason("FSCT service already run"));
        }
        service_impl.replace(new_service_abort_handle);
        Ok(())
    }

    #[napi]
    pub fn stop_fsct(&self) -> napi::Result<()> {
        let abort_handle = self
            .service_abort_handle.lock().unwrap()
            .take()
            .ok_or_else(|| napi::Error::from_reason("FSCT service not run"))?;
        abort_handle.abort();
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