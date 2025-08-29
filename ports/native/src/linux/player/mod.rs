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
use fsct_core::{FsctDriver, ManagedPlayerId, ServiceHandle, spawn_service};
use fsct_core::player_state::PlayerState;
use fsct_core::definitions::{FsctStatus, TimelineInfo};
use fsct_core::player_state::TrackMetadata;
use zbus::{Connection};
mod mpris;
use zbus::zvariant::{OwnedValue, Str, Array};

pub fn map_status(s: Option<&str>) -> FsctStatus {
    match s {
        Some("Playing") => FsctStatus::Playing,
        Some("Paused") => FsctStatus::Paused,
        Some("Stopped") => FsctStatus::Stopped,
        _ => FsctStatus::Unknown,
    }
}

pub fn extract_duration_from_metadata(metadata: &HashMap<String, OwnedValue>) -> Option<Duration> {
    if let Some(v) = metadata.get("mpris:length") {
        let us_total: Option<u64> = v.downcast_ref::<i64>().ok().map(|x| x as u64)
            .or_else(|| v.downcast_ref::<i32>().ok().map(|x| x as u64))
            .or_else(|| v.downcast_ref::<u64>().ok().map(|x| x));
        if let Some(us) = us_total { return Some(Duration::from_micros(us)); }
    }
    None
}

pub fn build_state_from_map(metadata: &HashMap<String, OwnedValue>, playback_status: Option<&str>, position_us: Option<i64>, playback_rate: Option<f64>) -> PlayerState {
    let mut texts = TrackMetadata::default();
    let mut duration: Option<Duration> = None;
    if let Some(v) = metadata.get("xesam:title") {
        if let Ok(sv) = v.downcast_ref::<Str>() { texts.title = Some(sv.to_string()); } else if let Ok(s) = v.downcast_ref::<String>() { texts.title = Some(s.clone()); }
    }
    if let Some(v) = metadata.get("xesam:artist") {
        if let Ok(arr) = v.downcast_ref::<Array>() {
            for elem in arr.iter() { // elem: Value
                if let Ok(sv) = elem.downcast_ref::<Str>() {
                    texts.artist = Some(sv.to_string());
                    break;
                }
                if let Ok(s) = elem.downcast_ref::<String>() {
                    texts.artist = Some(s.clone());
                    break;
                }
            }
        }
    }
    if let Some(v) = metadata.get("xesam:album") {
        if let Ok(sv) = v.downcast_ref::<Str>() { texts.album = Some(sv.to_string()); } else if let Ok(s) = v.downcast_ref::<String>() { texts.album = Some(s.clone()); }
    }
    if let Some(v) = metadata.get("mpris:length") {
        let us_total: Option<u64> = v.downcast_ref::<i64>().ok().map(|x| x as u64)
            .or_else(|| v.downcast_ref::<i32>().ok().map(|x| x as u64))
            .or_else(|| v.downcast_ref::<u64>().ok().map(|x| x));
        if let Some(us) = us_total { duration = Some(Duration::from_micros(us)); }
    }
    let pos = position_us.map(|us| Duration::from_micros(us as u64)).unwrap_or(Duration::from_secs(0));
    let status = map_status(playback_status);
    let rate = if matches!(status, FsctStatus::Playing) {
        playback_rate.unwrap_or(1.0)
    } else {
        0.0
    };

    PlayerState {
        status,
        texts,
        timeline: duration.map(|d| TimelineInfo { position: pos, update_time: SystemTime::now(), duration: d, rate }),
    }
}


pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    // Establish DBus session connection upfront; fail fast if it cannot be created.
    let conn = Connection::session().await?;

    // Install explicit match rules via DBus AddMatch using adapter
    mpris::install_match_rules(&conn).await?;

    let conn = conn.clone();
    let handle = spawn_service(move |mut stop| async move {
        // registry keyed by well-known name -> (id, state, last_position_us, owner_unique_name)
        let mut registry: HashMap<String, (ManagedPlayerId, PlayerState, Option<i64>, Option<String>)> = HashMap::new();

        // Initial discovery via adapter
        if let Ok(initials) = mpris::initial_players(&conn).await {
            for evt in initials {
                if let mpris::MprisEvent::PlayerAppeared { name, identity, mut state, owner } = evt {
                    if let Ok(id) = driver.register_player(format!("native-linux-mpris:{identity}.{name}")).await {
                        let _ = driver.update_player_state(id, state.clone()).await;
                        if let Some(mut tl) = state.timeline.clone() {
                            tl.update_time = SystemTime::now();
                            let rate: f64 = mpris::get_rate(&conn, name.as_str()).await.unwrap_or(1.0);
                            tl.rate = if matches!(state.status, FsctStatus::Playing) { rate } else { 0.0 };
                            let _ = driver.update_player_timeline(id, Some(tl.clone())).await;
                            state.timeline = Some(tl);
                        }
                        registry.insert(name, (id, state, None, owner));
                    }
                }
            }
        }

        // Consume events via channel to simplify select
        let mut evt_stream = mpris::MprisStream::new(conn.clone()).into_stream();
        use futures_util::StreamExt;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            futures_util::pin_mut!(evt_stream);
            while let Some(item) = evt_stream.next().await {
                if let Ok(evt) = item { let _ = tx.send(evt); }
            }
        });
        loop {
            tokio::select! {
                _ = stop.signaled() => break,
                Some(evt) = rx.recv() => {
                    match evt {
                        mpris::MprisEvent::PlayerAppeared { name, identity, mut state, owner } => {
                            if let Ok(id) = driver.register_player(format!("native-linux-mpris:{identity}.{name}")).await {
                                let _ = driver.update_player_state(id, state.clone()).await;
                                if let Some(mut tl) = state.timeline.clone() {
                                    tl.update_time = SystemTime::now();
                                    let rate: f64 = mpris::get_rate(&conn, name.as_str()).await.unwrap_or(1.0);
                                    tl.rate = if matches!(state.status, FsctStatus::Playing) { rate } else { 0.0 };
                                    let _ = driver.update_player_timeline(id, Some(tl.clone())).await;
                                    state.timeline = Some(tl);
                                }
                                registry.insert(name, (id, state, None, owner));
                            }
                        }
                        mpris::MprisEvent::PlayerDisappeared { name } => {
                            if let Some((id, _, _, _)) = registry.remove(&name) {
                                let _ = driver.update_player_state(id, PlayerState::default()).await;
                                let _ = driver.unregister_player(id).await;
                            }
                        }
                        mpris::MprisEvent::PropertiesChanged { name: _n, changed, sender } => {
                            for (_name, (id, last, _pos, owner)) in registry.iter_mut() {
                                if owner.as_ref().is_some() && sender.is_some() && owner.as_ref() != sender.as_ref() { continue; }
                                let status = changed.get("PlaybackStatus").and_then(|v| v.downcast_ref::<Str>().ok().map(|s| s.to_string()))
                                    .or_else(|| changed.get("PlaybackStatus").and_then(|v| v.downcast_ref::<String>().ok().map(|s| s.clone())));
                                if let Some(v) = changed.get("Rate") {
                                    if let Some(mut tl) = last.timeline.clone() {
                                        let mut rate_val = 1.0;
                                        if let Ok(val) = v.downcast_ref::<f64>() { rate_val = val; }
                                        else if let Ok(iv) = v.downcast_ref::<i64>() { rate_val = iv as f64; }
                                        else if let Ok(iv) = v.downcast_ref::<i32>() { rate_val = iv as f64; }
                                        tl.rate = if matches!(last.status, FsctStatus::Playing) { rate_val } else { 0.0 };
                                        tl.update_time = SystemTime::now();
                                        if let Ok(meta) = mpris::get_metadata(&conn, _name.as_str()).await {
                                            if let Some(new_dur) = extract_duration_from_metadata(&meta) {
                                                if new_dur != tl.duration { tl.duration = new_dur; }
                                            }
                                        }
                                        let _ = driver.update_player_timeline(*id, Some(tl.clone())).await;
                                        last.timeline = Some(tl);
                                    }
                                }
                                let mut new_state = last.clone();
                                if let Some(s) = status.as_deref() { new_state.status = map_status(Some(s)); }
                                if changed.contains_key("Metadata") {
                                    let well_known = _name.clone();
                                    let meta = mpris::get_metadata(&conn, well_known.as_str()).await.ok();
                                    // PlaybackStatus/Rate are optional; we can get them via proxies too, but we only need rate for timeline when Playing
                                    let status_str: Option<String> = None;
                                    let rate_opt: Option<f64> = mpris::get_rate(&conn, well_known.as_str()).await.ok();
                                    if let Some(meta_map) = meta.as_ref() {
                                        let prev_tl = last.timeline.clone();
                                        new_state = build_state_from_map(meta_map, status_str.as_deref(), None, rate_opt);
                                        if let (Some(_old_tl), Some(mut new_tl)) = (prev_tl, new_state.timeline.clone()) {
                                            let mut new_pos = Duration::from_secs(0);
                                            if let Ok(pos_us) = mpris::get_position(&conn, well_known.as_str()).await { new_pos = Duration::from_micros(pos_us as u64); }
                                            if new_pos > new_tl.duration { new_pos = new_tl.duration; }
                                            new_tl.position = new_pos;
                                            new_tl.update_time = SystemTime::now();
                                            let _ = driver.update_player_timeline(*id, Some(new_tl.clone())).await;
                                            new_state.timeline = Some(new_tl);
                                        }
                                    }
                                }
                                if *last != new_state {
                                    let prev = last.clone();
                                    *last = new_state.clone();
                                    let _ = driver.update_player_state(*id, new_state.clone()).await;
                                    if prev.status != last.status {
                                        if let Some(mut tl) = last.timeline.clone().or(prev.timeline.clone()) {
                                            if let Ok(pos_us) = mpris::get_position(&conn, _name.as_str()).await { tl.position = Duration::from_micros(pos_us as u64); }
                                            let mut rate_val: f64 = 1.0;
                                            if let Some(v) = changed.get("Rate") {
                                                if let Ok(val) = v.downcast_ref::<f64>() { rate_val = val; }
                                                else if let Ok(iv) = v.downcast_ref::<i64>() { rate_val = iv as f64; }
                                                else if let Ok(iv) = v.downcast_ref::<i32>() { rate_val = iv as f64; }
                                            } else if let Ok(r) = mpris::get_rate(&conn, _name.as_str()).await {
                                                rate_val = r;
                                            }
                                            tl.rate = if matches!(last.status, FsctStatus::Playing) { rate_val } else { 0.0 };
                                            tl.update_time = SystemTime::now();
                                            if let Ok(meta) = mpris::get_metadata(&conn, _name.as_str()).await {
                                                if let Some(new_dur) = extract_duration_from_metadata(&meta) { if new_dur != tl.duration { tl.duration = new_dur; } }
                                            }
                                            let _ = driver.update_player_timeline(*id, Some(tl.clone())).await;
                                            last.timeline = Some(tl);
                                        }
                                    }
                                }
                            }
                        }
                        mpris::MprisEvent::Seeked { name: _n, pos_us, sender } => {
                            for (_name, (id, last, pos_slot, owner)) in registry.iter_mut() {
                                if owner.as_ref().is_some() && sender.as_ref().is_some() && owner.as_ref() != sender.as_ref() { continue; }
                                *pos_slot = Some(pos_us);
                                if let Some(mut tl) = last.timeline.clone() {
                                    tl.position = Duration::from_micros(pos_us as u64);
                                    tl.update_time = SystemTime::now();
                                    if let Ok(meta) = mpris::get_metadata(&conn, _name.as_str()).await {
                                        if let Some(new_dur) = extract_duration_from_metadata(&meta) { if new_dur != tl.duration { tl.duration = new_dur; } }
                                    }
                                    let _ = driver.update_player_timeline(*id, Some(tl.clone())).await;
                                    last.timeline = Some(tl);
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    Ok(handle)
}
