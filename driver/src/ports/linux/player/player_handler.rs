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

use crate::ports::linux::player::mpris::{PlaybackStatus, Player, PlayerProxy};
use fsct::definitions::{FsctStatus, ManagedPlayerId, TimelineInfo};
use fsct::player_state::TrackMetadata;
use fsct::{FsctDriver, PlayerState};
use futures_util::StreamExt;
use log::{debug, warn};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::select;
use zbus::export::ordered_stream::OrderedStreamExt;
use zbus::zvariant::OwnedValue;

pub struct PlayerHandler {
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
    pub fn new(player: Player, id: ManagedPlayerId, driver: Arc<dyn FsctDriver>) -> Self {
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
    fn now() -> SystemTime {
        SystemTime::now()
    }

    fn micros_to_duration(v: i64) -> Duration {
        if v <= 0 {
            Duration::from_micros(0)
        } else {
            Duration::from_micros(v as u64)
        }
    }

    fn effective_rate(status: FsctStatus, rate_opt: Option<f64>) -> f64 {
        match status {
            FsctStatus::Playing => rate_opt.unwrap_or(1.0),
            _ => 0.0,
        }
    }

    fn parse_metadata(
        map: &std::collections::HashMap<String, OwnedValue>,
    ) -> (fsct::player_state::TrackMetadata, Option<Duration>) {
        let mut md = TrackMetadata::default();
        let mut dur: Option<Duration> = None;
        for (name, value) in map.iter() {
            match name.as_str() {
                "xesam:title" => md.title = Self::parse_text(value),
                "xesam:artist" => md.artist = Self::parse_artists(value),
                "xesam:album" => md.album = Self::parse_text(value),
                "xesam:genre" => md.genre = Self::parse_text(value),
                "mpris:length" => dur = Self::parse_length(value),
                _ => (),
            }
        }
        (md, dur)
    }

    fn parse_artists(value: &OwnedValue) -> Option<String> {
        let one_artist = Self::parse_text(value);
        let artists = if let Some(one_artist) = one_artist {
            Some(one_artist)
        } else {
            Self::parse_text_array(value)
        };
        artists
    }

    fn parse_text_array(value: &OwnedValue) -> Option<String> {
        let multiple_texts: Option<Vec<String>> = value.clone().try_into().ok();
        if let Some(multiple_texts) = multiple_texts {
            Some(multiple_texts.join(", "))
        } else {
            None
        }
    }

    fn parse_text(value: &OwnedValue) -> Option<String> {
        value.clone().try_into().ok()
    }

    fn parse_length(value: &OwnedValue) -> Option<Duration> {
        let duration = if let Ok(us) = <u64 as TryFrom<OwnedValue>>::try_from(value.clone()) {
            Some(Duration::from_micros(us))
        } else if let Ok(us) = <i64 as TryFrom<OwnedValue>>::try_from(value.clone()) {
            Some(Self::micros_to_duration(us))
        } else if let Ok(x) = <i32 as TryFrom<OwnedValue>>::try_from(value.clone()) {
            Some(Self::micros_to_duration(x as i64))
        } else {
            None
        };
        duration
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

    pub async fn handle_player_task(&self) -> anyhow::Result<()> {
        let player_proxy = self.player.as_player_interface().await?;
        self.build_initial_state(&player_proxy).await?;
        let initial_state = { self.state.lock().unwrap().clone() };
        debug!("Player {} is ready, initial state: {:?}", self.id, initial_state);
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
                    debug!("Playback status (prop: {}) changed: {:?}", res.name(), value);
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
                    debug!("Rate changed: {}", rate_val);
                    self.update_rate(Some(rate_val)).await;
                }
                Err(e) => warn!("Error receiving rate: {}", e),
            }
        }
    }

    async fn handle_seeked_task<'a>(&'a self, player_proxy: &PlayerProxy<'a>) -> anyhow::Result<()> {
        let mut sig = player_proxy
            .receive_seeked()
            .await
            .inspect_err(|e| warn!("Error subscribing to seek signal: {}", e))?;
        while let Some(event) = OrderedStreamExt::next(&mut sig).await {
            match event.args() {
                Ok(args) => {
                    let pos_us = args.Position();
                    debug!("Seeked to: {}", pos_us);
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
                    debug!("Metadata changed: {:?}", map);
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
        for (ty, opt) in texts.iter() {
            // iterator yields (FsctTextMetadata, &Option<String>)
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
        if let Some(duration) = parts.duration
            && let Some(position) = parts.position
        {
            Some(TimelineInfo {
                duration,
                position,
                update_time: parts.update_time,
                rate: Self::effective_rate(status, parts.rate),
            })
        } else {
            None
        }
    }

    fn recalculate_position(&self) {
        let status = self.state.lock().unwrap().status;
        let mut timeline_parts = self.timeline_parts.lock().unwrap();
        if let Some(position) = timeline_parts.position {
            let now = Self::now();
            let elapsed = now
                .duration_since(timeline_parts.update_time)
                .unwrap_or(Duration::from_micros(0));
            let advance = (elapsed.as_micros() as f64) * Self::effective_rate(status, timeline_parts.rate);
            let advance = Duration::from_micros(advance.max(0.0) as u64); // don't advance backwards
            let pos = position + advance;
            timeline_parts.position = Some(pos);
            timeline_parts.update_time = now;
        }
    }
}
