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

use std::fmt::format;
use std::sync::{Arc, Mutex};
use anyhow::bail;
use futures_util::future::select;
use futures_util::{StreamExt, TryFutureExt};
use log::{info, warn};
use tokio::select;
use tokio::task::JoinSet;
use fsct_core::{spawn_service, FsctDriver, ManagedPlayerId, PlayerState, ServiceHandle};
use tokio_util::sync::{CancellationToken, DropGuard};
use zbus::export::ordered_stream::OrderedStreamExt;
use fsct_core::definitions::FsctStatus;

mod mpris;
use mpris::*;

struct PlayerRegistrationManager {
    driver: Arc<dyn FsctDriver>,
    cancellation_token: CancellationToken,
    drop_guard: DropGuard,
}

impl PlayerRegistrationManager {
    fn new(driver: Arc<dyn FsctDriver>) -> Self {
        let cancellation_token = CancellationToken::new();
        let drop_guard = cancellation_token.clone().drop_guard();
        Self { driver, cancellation_token, drop_guard }
    }

    async fn register_player(&self, player: mpris::Player) -> Result<(), anyhow::Error> {
        let id = self.driver.register_player(player.name()).await?;
        let cancel_token = self.cancellation_token.child_token();
        info!("Registered player: {}", id);
        let driver = self.driver.clone();
        tokio::spawn(async move {
            let player_handler = PlayerHandler::new(player, id, driver.clone());
            select! {
                _ = cancel_token.cancelled() => {},
                _ = player_handler.handle_player_task() => {}
            }
            let _ = driver.unregister_player(id).await;
            info!("Unregistered player: {}", id);
        });
        Ok(())
    }
}

impl From<PlaybackStatus> for FsctStatus {
    fn from(status: PlaybackStatus) -> Self {
        match status {
            PlaybackStatus::Playing => FsctStatus::Playing,
            PlaybackStatus::Paused => FsctStatus::Paused,
            PlaybackStatus::Stopped => FsctStatus::Stopped,
        }
    }
}

use std::time::{Duration, SystemTime};
use zbus::zvariant::OwnedValue;
use fsct_core::definitions::{TimelineInfo, FsctTextMetadata};
use fsct_core::player_state::TrackMetadata;

struct PlayerHandler {
    player: Player,
    id: ManagedPlayerId,
    driver: Arc<dyn FsctDriver>,
    state: Mutex<PlayerState>,
    timeline_parts: Mutex<TimelineParts>,
}

#[derive(Clone)]
struct TimelineParts {
    position: Option<Duration>,
    rate: Option<f64>,
    duration: Option<Duration>,
    update_time: SystemTime,
}

impl PlayerHandler {
    fn new(player: Player, id: ManagedPlayerId, driver: Arc<dyn FsctDriver>) -> Self {
        Self {
            player,
            id,
            driver,
            timeline_parts: Mutex::new(TimelineParts {
                position: None,
                rate: None,
                duration: None,
                update_time: SystemTime::now(),
            }),
            state: Mutex::default(),
        }
    }

    // --- Helpers ---
    fn now() -> SystemTime { SystemTime::now() }

    fn micros_to_duration(v: i64) -> Duration {
        if v <= 0 { Duration::from_micros(0) } else { Duration::from_micros(v as u64) }
    }

    fn effective_rate(status: FsctStatus, rate_opt: Option<f64>) -> f64 {
        match status {
            FsctStatus::Playing => rate_opt.unwrap_or(1.0),
            _ => 0.0,
        }
    }

    fn parse_metadata(map: &std::collections::HashMap<String, OwnedValue>) -> (fsct_core::player_state::TrackMetadata, Option<Duration>) {
        use zbus::zvariant::*;
        let mut md = fsct_core::player_state::TrackMetadata::default();
        let mut dur: Option<Duration> = None;
        for (name, value) in map.iter() {
            match name.as_str() {
                "xesam:title" => md.title = value.clone().try_into().ok(),
                "xesam:artist" => {
                    let one_artist = value.clone().try_into().ok();
                    if let Some(one_artist) = one_artist {
                        md.artist = Some(one_artist);
                    } else {
                        let multiple_artists: Option<Vec<String>> = value.clone().try_into().ok(); //Vec::<String>::try_from(value).ok();
                        if let Some(mulitple_artists) = multiple_artists {
                            md.artist = Some(mulitple_artists.join(", "));
                        }
                    }
                }
                "xesam:album" => md.album = value.clone().try_into().ok(),
                "xesam:genre" => md.genre = value.clone().try_into().ok(),
                "mpris:length" => {
                    if let Ok(us) = <i64 as TryFrom<OwnedValue>>::try_from(value.clone()) {
                        dur = Some(Self::micros_to_duration(us));
                    } else if let Ok(x) = <i32 as TryFrom<OwnedValue>>::try_from(value.clone()) {
                        dur = Some(Self::micros_to_duration(x as i64));
                    }
                }
                _ => ()
            }
        }
        (md, dur)
    }

    async fn build_initial_state<'a>(&self, player_proxy: &PlayerProxy<'a>) -> anyhow::Result<()> {
        let status: FsctStatus = player_proxy.playback_status().await?.into();
        let rate = player_proxy.rate().await.ok();
        let position = player_proxy.position().await.map(Self::micros_to_duration).ok();
        let now = Self::now();
        let metadata = player_proxy.metadata().await?;
        let (texts, duration) = Self::parse_metadata(&metadata);

        let timeline_parts = TimelineParts {
            position,
            rate,
            duration,
            update_time: now,
        };

        let timeline = Self::get_timeline(&timeline_parts, status);

        *self.timeline_parts.lock().unwrap() = timeline_parts;

        let mut state = self.state.lock().unwrap();
        state.status = status;
        state.texts = texts;
        state.timeline = timeline;
        Ok(())
    }

    async fn handle_player_task(&self) -> anyhow::Result<()> {
        let player_proxy = self.player.as_player_interface().await?;
        self.build_initial_state(&player_proxy).await?;
        let initial_state = { self.state.lock().unwrap().clone() };
        self.driver.update_player_state(self.id, initial_state).await?;
        select! {
            res = self.player.wait_for_disconnect() => {
                return res;
            }
            _ = self.handle_playback_status_changed_task(&player_proxy) => {}
            _ = self.handle_rate_changed_task(&player_proxy) => {}
            _ = self.handle_seeked_task(&player_proxy) => {}
            _ = self.handle_metadata_changed_task(&player_proxy) => {}
        }
        Ok(())
    }

    async fn handle_playback_status_changed_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) {
        let mut playback_status_changed_signal = player_proxy.receive_playback_status_changed().await;
        while let Some(res) = playback_status_changed_signal.next().await {
            let value = res.get().await;
            match value {
                Ok(value) => {
                    info!("Playback status (prop: {}) changed: {:?}", res.name(), value);
                    self.update_status(value).await;
                }
                Err(e) => {
                    warn!("Error receiving playback status: {}", e);
                    let _ = self.driver.update_player_status(self.id, FsctStatus::Error).await;
                }
            }
        }
    }

    async fn update_status(&self, value: PlaybackStatus) {
        // before we update the status, we need to recalculate position at the current timepoint
        self.recalculate_position();
        let new_status: FsctStatus = value.into();
        self.state.lock().unwrap().status = new_status;
        let _ = self.driver.update_player_status(self.id, new_status).await;
        // but we update the timeline at the end so that the timeline is updated with the new status
        self.update_timeline().await;
    }

    async fn handle_rate_changed_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) {
        let mut sig = player_proxy.receive_rate_changed().await;
        while let Some(res) = sig.next().await {
            match res.get().await {
                Ok(rate_val) => {
                    info!("Rate changed: {}", rate_val);
                    self.update_rate(Some(rate_val)).await;
                }
                Err(e) => warn!("Error receiving rate: {}", e),
            }
        }
    }

    async fn handle_seeked_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) -> anyhow::Result<()> {
        let mut sig = player_proxy.receive_seeked().await
            .inspect_err(|e| warn!("Error subscribing to seek signal: {}", e))?;
        while let Some(event) = OrderedStreamExt::next(&mut sig).await {
            match event.args() {
                Ok(args) => {
                    let pos_us = args.Position();
                    info!("Seeked to: {}", pos_us);
                    self.update_position(Some(Self::micros_to_duration(*pos_us))).await;
                }
                Err(e) => warn!("Error receiving position: {}", e),
            }
        }
        Ok(())
    }

    async fn handle_metadata_changed_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) {
        let mut sig = player_proxy.receive_metadata_changed().await;
        while let Some(res) = sig.next().await {
            match res.get().await {
                Ok(map) => {
                    info!("Metadata changed: {:?}", map);
                    let (texts, duration) = Self::parse_metadata(&map);
                    self.update_texts(texts).await;
                    self.update_duration(duration).await;
                }
                Err(e) => warn!("Error receiving metadata: {}", e),
            }
        }
    }

    async fn update_texts(&self, texts: TrackMetadata) {
        self.state.lock().unwrap().texts = texts.clone();
        for (ty, opt) in texts.iter() { // iterator yields (FsctTextMetadata, &Option<String>)
            let _ = self.driver.update_player_metadata(self.id, ty, opt.clone()).await;
        }
    }

    async fn update_duration(&self, duration: Option<Duration>) {
        self.timeline_parts.lock().unwrap().duration = duration;
        self.update_timeline().await;
    }

    async fn update_position(&self, position: Option<Duration>) {
        {
            let mut parts = self.timeline_parts.lock().unwrap();
            parts.position = position;
            parts.update_time = Self::now();
        }
        self.update_timeline().await;
    }

    async fn update_rate(&self, rate: Option<f64>) {
        // before we update the rate, we need to recalculate the timeline to have position calculated at the current timepoint
        self.recalculate_position();
        self.timeline_parts.lock().unwrap().rate = rate;
        self.update_timeline().await;
    }

    async fn update_timeline(&self) {
        let new_timeline = {
            let parts = self.timeline_parts.lock().unwrap().clone();
            let mut state = self.state.lock().unwrap();
            let status = state.status;
            let timeline = Self::get_timeline(&parts, status);
            if timeline != state.timeline {
                state.timeline = timeline.clone();
                Some(timeline)
            } else {
                None
            }
        };
        if let Some(timeline) = new_timeline {
            let _ = self.driver.update_player_timeline(self.id, timeline).await;
        }
    }

    fn get_timeline(parts: &TimelineParts, status: FsctStatus) -> Option<TimelineInfo> {
        if let Some(duration) = parts.duration && let Some(position) = parts.position {
            Some(TimelineInfo {
                duration,
                position,
                update_time: parts.update_time,
                rate: Self::effective_rate(status, parts.rate),
            })
        } else { None }
    }

    fn recalculate_position(&self) {
        let status = self.state.lock().unwrap().status;
        let mut timeline_parts = self.timeline_parts.lock().unwrap();
        if let Some(position) = timeline_parts.position {
            let now = Self::now();
            let elapsed = now.duration_since(timeline_parts.update_time).unwrap_or(Duration::from_micros(0));
            let advance = (elapsed.as_micros() as f64) * Self::effective_rate(status, timeline_parts.rate);
            let advance = Duration::from_micros(advance.max(0.0) as u64); // don't advance backwards
            let pos = position + advance;
            timeline_parts.position = Some(pos);
            timeline_parts.update_time = now;
        }
    }
}

pub async fn run_os_player_watcher(driver: Arc<dyn FsctDriver>, player_watcher: mpris::SessionWatcher) -> anyhow::Result<()> {
    let manager = PlayerRegistrationManager::new(driver.clone());
    let mut stream = player_watcher.iter_player(true).await;
    futures_util::pin_mut!(stream);
    while let Some(player) = stream.next().await {
        match player {
            Ok(player) => {
                let player_registration_result = manager.register_player(player).await;
                if let Err(e) = player_registration_result {
                    warn!("Error registering player: {}", e);
                }
            }
            Err(e) => warn!("Error connecting to player: {}", e),
        }
    }
    bail!("Player watcher stopped unexpectedly");
}

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    let player_watcher = mpris::SessionWatcher::new().await?;
    let handle = spawn_service(|mut stop| async move {
        select! {
            _ = stop.signaled() => {},
            _ = run_os_player_watcher(driver.clone(), player_watcher) => {},
        }
    });
    Ok(handle)
}
