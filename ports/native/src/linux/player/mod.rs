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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use anyhow::anyhow;
use fsct_core::{FsctDriver, ManagedPlayerId, ServiceHandle, spawn_service};
use fsct_core::player_state::PlayerState;
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::player_state::TrackMetadata;
use log::warn;
use mpris::{Player, PlayerFinder, Metadata, PlaybackStatus};

fn state_from_metadata_and_status(metadata: &Metadata, status: Option<PlaybackStatus>, position: Option<Duration>) -> PlayerState {
    let mut texts = TrackMetadata::default();
    texts.title = metadata.title().map(|s| s.to_string());
    texts.artist = metadata.artists().and_then(|v| v.get(0).map(|s| s.to_string()));
    texts.album = metadata.album_name().map(|s| s.to_string());

    let timeline = metadata.length().map(|len| {
        let pos = position.unwrap_or(Duration::from_secs(0));
        TimelineInfo {
            position: pos,
            update_time: SystemTime::now(),
            duration: len,
            rate: match status {
                Some(PlaybackStatus::Playing) => 1.0,
                Some(PlaybackStatus::Paused) => 0.0,
                _ => 0.0,
            },
        }
    });

    let fsct_status = match status {
        Some(PlaybackStatus::Playing) => FsctStatus::Playing,
        Some(PlaybackStatus::Paused) => FsctStatus::Paused,
        Some(PlaybackStatus::Stopped) | None => FsctStatus::Stopped,
    };

    PlayerState { status: fsct_status, texts, timeline }
}

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    // Discover MPRIS players and register either one or many. For simplicity and robustness on Linux,
    // we will register all detected players and track them; the Orchestrator can pick the best. 
    let finder = PlayerFinder::new().map_err(|e| anyhow!(e))?;

    let players: Vec<Player> = match finder.find_all() {
        Ok(list) => list,
        Err(e) => {
            // If discovery fails, start with an empty set and rely on periodic refresh
            warn!("Failed to discover MPRIS players initially: {:?}", e);
            Vec::new()
        },
    };

    // Initial registration using a temporary local registry; long-running registry is in the async task
    for p in players {
        let identity = p.identity();
        let bus_name = p.bus_name().to_string();
        let fsct_id = driver.register_player(format!("native-linux-mpris:{}.{}", identity, bus_name)).await?;
        // initial state
        let md = p.get_metadata().ok();
        let status = p.get_playback_status().ok();
        let pos = p.get_position().ok();
        let state = if let Some(md) = md { state_from_metadata_and_status(&md, status, pos) } else { PlayerState::default() };
        let _ = driver.update_player_state(fsct_id, state.clone()).await;
    }

    // Spawn watcher task using a helper thread for non-Send MPRIS types; the async task diffs and updates driver
    let handle = spawn_service(move |mut stop| async move {
        use tokio::sync::mpsc;
        #[derive(Clone)]
        struct ReportedPlayer { bus: String, identity: String, state: PlayerState }
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<ReportedPlayer>>();

        // Start blocking thread that owns MPRIS connection
        std::thread::spawn(move || {
            let maybe_finder = PlayerFinder::new();
            if maybe_finder.is_err() {
                warn!("Failed to create PlayerFinder: {:?}", maybe_finder.err());
                return;
            }
            let finder = maybe_finder.unwrap();
            loop {
                match finder.find_all() {
                    Ok(players) => {
                        let mut batch: Vec<ReportedPlayer> = Vec::with_capacity(players.len());
                        for p in players {
                            let bus = p.bus_name().to_string();
                            let identity = p.identity().to_string();
                            let md = p.get_metadata().ok();
                            let status = p.get_playback_status().ok();
                            let pos = p.get_position().ok();
                            let state = if let Some(md) = md { state_from_metadata_and_status(&md, status, pos) } else { PlayerState::default() };
                            batch.push(ReportedPlayer { bus, identity, state });
                        }
                        let _ = tx.send(batch);
                    }
                    Err(e) => {
                        warn!("MPRIS discovery failed: {:?}", e);
                        let _ = tx.send(Vec::new());
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(750));
            }
        });

        // Async side registry (bus -> (fsct_id, last_state))
        let mut registry: HashMap<String, (ManagedPlayerId, PlayerState)> = HashMap::new();

        while let Some(batch) = tokio::select! {
            _ = stop.signaled() => None,
            b = rx.recv() => b,
        } {
            // mark seen
            let mut seen: HashMap<String, ()> = HashMap::new();
            for rp in batch.into_iter() {
                seen.insert(rp.bus.clone(), ());
                if let Some((fsct_id, last)) = registry.get_mut(&rp.bus) {
                    if *last != rp.state {
                        *last = rp.state.clone();
                        let _ = driver.update_player_state(*fsct_id, rp.state).await;
                    }
                } else {
                    // new player
                    let name = format!("native-linux-mpris:{}.{}", rp.identity, rp.bus);
                    if let Ok(id) = driver.register_player(name).await {
                        let _ = driver.update_player_state(id, rp.state.clone()).await;
                        registry.insert(rp.bus, (id, rp.state));
                    }
                }
            }
            // handle removed
            let removed: Vec<String> = registry.keys().filter(|k| !seen.contains_key(*k)).cloned().collect();
            for bus in removed {
                if let Some((id, _)) = registry.remove(&bus) {
                    let _ = driver.update_player_state(id, PlayerState::default()).await;
                }
            }
        }
    });

    Ok(handle)
}
