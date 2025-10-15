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


// Example: LoggingDriver that prints all driver interactions to stdout/stderr
// Run with:
//   cargo run --package fsct_driver --example logging_driver
// On Linux this will wire to the MPRIS-based OS watcher; on Windows/macOS it uses their respective watchers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Error;
use async_trait::async_trait;
use env_logger::Env;
use tokio::sync::broadcast::Receiver;
use fsct::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};
use fsct::driver::FsctDriver;
use fsct::definitions::ManagedPlayerId;
use fsct::definitions::ManagedDeviceId;
use fsct::DeviceChangeEvent;
use fsct::player_state::PlayerState;
use fsct_driver::joinable_task::JoinableTaskHandle;

use fsct_driver::run_os_watcher;


#[derive(Default)]
struct LoggingDriver {
    players: Mutex<HashMap<ManagedPlayerId, String>>, // id -> name
    next_id: Mutex<u32>,
}

impl LoggingDriver {
    fn new() -> Self { Self { players: Mutex::new(HashMap::new()), next_id: Mutex::new(1) } }
    fn alloc_id(&self) -> ManagedPlayerId {
        let mut n = self.next_id.lock().unwrap();
        let id = *n;
        *n += 1;
        std::num::NonZeroU32::new(id).expect("ManagedPlayerId must be non-zero")
    }
}

#[async_trait]
impl FsctDriver for LoggingDriver {
    async fn register_player(&self, self_id: String) -> Result<ManagedPlayerId, Error> {
        let id = self.alloc_id();
        self.players.lock().unwrap().insert(id, self_id.clone());
        println!("[LoggingDriver] register_player: id={:?}, name={}", id, self_id);
        Ok(id)
    }

    async fn unregister_player(&self, player_id: ManagedPlayerId) -> Result<(), Error> {
        let removed = self.players.lock().unwrap().remove(&player_id);
        println!("[LoggingDriver] unregister_player: id={:?}, existed={}", player_id, removed.is_some());
        Ok(())
    }

    async fn assign_player_to_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        println!("[LoggingDriver] assign_player_to_device: player={:?} -> device={:?}", player_id, device_id);
        Ok(())
    }

    async fn unassign_player_from_device(&self, player_id: ManagedPlayerId, device_id: ManagedDeviceId) -> Result<(), Error> {
        println!("[LoggingDriver] unassign_player_from_device: player={:?} -/-> device={:?}", player_id, device_id);
        Ok(())
    }

    async fn update_player_state(&self, player_id: ManagedPlayerId, new_state: PlayerState) -> Result<(), Error> {
        let name = self.players.lock().unwrap().get(&player_id).cloned().unwrap_or_else(|| "<unknown>".into());
        println!("[LoggingDriver] update_player_state: id={:?} ({}) => {:?}", player_id, name, new_state);
        Ok(())
    }

    async fn update_player_status(&self, player_id: ManagedPlayerId, new_status: FsctStatus) -> Result<(), Error> {
        let name = self.players.lock().unwrap().get(&player_id).cloned().unwrap_or_else(|| "<unknown>".into());
        println!("[LoggingDriver] update_player_status: id={:?} ({}) => {:?}", player_id, name, new_status);
        Ok(())
    }

    async fn update_player_timeline(&self, player_id: ManagedPlayerId, new_timeline: Option<TimelineInfo>) -> Result<(), Error> {
        let name = self.players.lock().unwrap().get(&player_id).cloned().unwrap_or_else(|| "<unknown>".into());
        println!("[LoggingDriver] update_player_timeline: id={:?} ({}) => {:?}", player_id, name, new_timeline);
        Ok(())
    }

    async fn update_player_metadata(&self, player_id: ManagedPlayerId, metadata_id: FsctTextMetadata, new_text: Option<String>) -> Result<(), Error> {
        let name = self.players.lock().unwrap().get(&player_id).cloned().unwrap_or_else(|| "<unknown>".into());
        println!("[LoggingDriver] update_player_metadata: id={:?} ({}), meta={:?} => {:?}", player_id, name, metadata_id, new_text);
        Ok(())
    }

    async fn get_player_assigned_device(&self, player_id: ManagedPlayerId) -> Result<Option<ManagedDeviceId>, Error> {
        println!("[LoggingDriver] get_player_assigned_device: id={:?}", player_id);
        Ok(None)
    }

    async fn get_detected_devices(&self) -> Result<Vec<fsct::definitions::DeviceInfo>, Error> {
        println!("[LoggingDriver] get_detected_devices");
        Ok(Vec::new())
    }

    async fn subscribe_device_changes(&self) -> Result<Receiver<DeviceChangeEvent>, Error> {
        todo!()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let env = Env::default().filter_or("FSCT_LOG", "info").write_style("FSCT_LOG_STYLE");
    env_logger::init_from_env(env);

    let driver: Arc<dyn FsctDriver> = Arc::new(LoggingDriver::new());

    // Start the OS watcher for the current platform
    let watcher: JoinableTaskHandle = run_os_watcher(driver.clone()).await?;

    // Wait for Ctrl+C, then shut down
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
    }

    // Stop background watcher
    watcher.shutdown().await.ok();
    Ok(())
}
