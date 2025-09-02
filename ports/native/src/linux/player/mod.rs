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

struct PlayerHandler {
    player: Player,
    id: ManagedPlayerId,
    driver: Arc<dyn FsctDriver>,
    state: Mutex<PlayerState>,
}

impl PlayerHandler {
    fn new(player: Player, id: ManagedPlayerId, driver: Arc<dyn FsctDriver>) -> Self {
        Self {
            player,
            id,
            driver,
            state: Mutex::new(PlayerState::default()),
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

    fn recalc_position(old_tl: &TimelineInfo, now: SystemTime) -> Duration {
        let elapsed = now.duration_since(old_tl.update_time).unwrap_or(Duration::from_micros(0));
        let advance = (elapsed.as_micros() as f64) * old_tl.rate;
        let advance = Duration::from_micros(advance.max(0.0) as u64);
        let mut pos = old_tl.position + advance;
        if pos > old_tl.duration { pos = old_tl.duration; }
        pos
    }

    fn parse_metadata(map: &std::collections::HashMap<String, OwnedValue>) -> (fsct_core::player_state::TrackMetadata, Option<Duration>) {
        use zbus::zvariant::*;
        let mut md = fsct_core::player_state::TrackMetadata::default();
        let mut dur: Option<Duration> = None;
        if let Some(v) = map.get("mpris:length") {
            if let Ok(us) = <i64 as TryFrom<OwnedValue>>::try_from(v.clone()) {
                dur = Some(Self::micros_to_duration(us));
            } else if let Ok(x) = <i32 as TryFrom<OwnedValue>>::try_from(v.clone()) {
                dur = Some(Self::micros_to_duration(x as i64));
            }
        }
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
                _ => ()
                // if let Ok(s) = <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(value.clone()) { md.title = Some(s); }
            }
        }
        // if let Some(v) = map.get("xesam:title") {
        //     if let Ok(s) = <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(v.clone()) { md.title = Some(s); }
        // }
        // if let Some(v) = map.get("xesam:artist") {
        //     if let Ok(s) = <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(v.clone()) { md.artist = Some(s); }
        // }
        // if let Some(v) = map.get("xesam:album") {
        //     if let Ok(s) = <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(v.clone()) { md.album = Some(s); }
        // }
        // if let Some(v) = map.get("xesam:genre") {
        //     if let Ok(s) = <String as TryFrom<zbus::zvariant::OwnedValue>>::try_from(v.clone()) { md.genre = Some(s); }
        // }
        (md, dur)
    }

    async fn build_initial_state<'a>(&self, player_proxy: &PlayerProxy<'a>) -> anyhow::Result<()> {
        let status: FsctStatus = player_proxy.playback_status().await?.into();
        let rate_prop = player_proxy.rate().await.ok();
        let pos_us = player_proxy.position().await.unwrap_or(0);
        let metadata = player_proxy.metadata().await?;
        let (texts, duration_opt) = Self::parse_metadata(&metadata);

        let now = Self::now();
        let position = Self::micros_to_duration(pos_us);
        let duration = duration_opt.unwrap_or(Duration::from_micros(0));
        let rate = Self::effective_rate(status, rate_prop);
        let timeline = Some(TimelineInfo { position, update_time: now, duration, rate });

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
            _ = self.handle_position_changed_task(&player_proxy) => {}
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
                    let new_status: FsctStatus = value.into();
                    // Update status and recalc timeline based on previous timeline rate
                    let rate_opt = player_proxy.rate().await.ok();
                    let (new_timeline, new_status_copy) = {
                        let mut st = self.state.lock().unwrap();
                        let now = Self::now();
                        let mut new_tl = st.timeline.clone();
                        if let Some(old_tl) = st.timeline.clone() { // todo remove some clones
                            let new_pos = Self::recalc_position(&old_tl, now);
                            let new_rate = Self::effective_rate(new_status, rate_opt);
                            new_tl = Some(TimelineInfo { position: new_pos, update_time: now, duration: old_tl.duration, rate: new_rate });
                            st.timeline = new_tl.clone();
                        }
                        st.status = new_status;
                        (new_tl, st.status)
                    };
                    if new_timeline.is_some() { let _ = self.driver.update_player_timeline(self.id, new_timeline).await; }
                    let _ = self.driver.update_player_status(self.id, new_status_copy).await;
                }
                Err(e) => {
                    warn!("Error receiving playback status: {}", e);
                    let _ = self.driver.update_player_status(self.id, FsctStatus::Error).await;
                }
            }
        }
    }

    async fn handle_rate_changed_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) {
        let mut sig = player_proxy.receive_rate_changed().await;
        while let Some(res) = sig.next().await {
            match res.get().await {
                Ok(rate_val) => {
                    info!("Rate changed: {}", rate_val);
                    let new_timeline = {
                        let mut st = self.state.lock().unwrap();
                        let now = Self::now();
                        if let Some(old_tl) = st.timeline.clone() { //todo less clones
                            let new_pos = Self::recalc_position(&old_tl, now);
                            let eff = Self::effective_rate(st.status, Some(rate_val));
                            let tl = Some(TimelineInfo { position: new_pos, update_time: now, duration: old_tl.duration, rate: eff });
                            st.timeline = tl.clone();
                            tl
                        } else { None }
                    };
                    if new_timeline.is_some() { let _ = self.driver.update_player_timeline(self.id, new_timeline).await; }
                }
                Err(e) => warn!("Error receiving rate: {}", e),
            }
        }
    }

    async fn handle_position_changed_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) {
        let mut sig = player_proxy.receive_position_changed().await;
        while let Some(res) = sig.next().await {
            match res.get().await {
                Ok(pos_us) => {
                    let rate_opt = player_proxy.rate().await.ok();
                    let new_timeline = {
                        let mut st = self.state.lock().unwrap();
                        let now = Self::now();
                        let position = Self::micros_to_duration(pos_us);
                        let eff = Self::effective_rate(st.status, rate_opt);
                        let duration = st.timeline.as_ref().map(|t| t.duration).unwrap_or(Duration::from_micros(0));
                        let tl = Some(TimelineInfo { position, update_time: now, duration, rate: eff });
                        st.timeline = tl.clone();
                        tl
                    };
                    let _ = self.driver.update_player_timeline(self.id, new_timeline).await;
                }
                Err(e) => warn!("Error receiving position: {}", e),
            }
        }
    }

    async fn handle_metadata_changed_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) {
        let mut sig = player_proxy.receive_metadata_changed().await;
        while let Some(res) = sig.next().await {
            match res.get().await {
                Ok(map) => {
                    info!("Metadata changed");
                    let (texts, duration_opt) = Self::parse_metadata(&map);
                    // Update texts individually (drop lock before awaits)
                    for (ty, opt) in texts.iter() { // iterator yields (FsctTextMetadata, &Option<String>)
                        let _ = self.driver.update_player_metadata(self.id, ty, opt.clone()).await;
                    }
                    // Reset timeline position to 0 and update duration
                    let rate_opt = player_proxy.rate().await.ok();
                    let new_timeline = {
                        let mut st = self.state.lock().unwrap();
                        st.texts = texts;
                        let now = Self::now();
                        let duration = duration_opt.or(st.timeline.as_ref().map(|t| t.duration)).unwrap_or(Duration::from_micros(0));
                        let eff = Self::effective_rate(st.status, rate_opt);
                        let tl = Some(TimelineInfo { position: Duration::from_micros(0), update_time: now, duration, rate: eff });
                        st.timeline = tl.clone();
                        tl
                    };
                    let _ = self.driver.update_player_timeline(self.id, new_timeline).await;
                }
                Err(e) => warn!("Error receiving metadata: {}", e),
            }
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
