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
use log::warn;
use zbus::{Connection, MessageStream, MatchRule};
use zbus::names::BusName;
use zbus::proxy::Proxy;
use zbus::fdo::DBusProxy;
use zbus::zvariant::{OwnedValue, Str, Array};

fn map_status(s: Option<&str>) -> FsctStatus {
    match s {
        Some("Playing") => FsctStatus::Playing,
        Some("Paused") => FsctStatus::Paused,
        Some("Stopped") => FsctStatus::Stopped,
        _ => FsctStatus::Unknown,
    }
}

fn extract_duration_from_metadata(metadata: &HashMap<String, OwnedValue>) -> Option<Duration> {
    if let Some(v) = metadata.get("mpris:length") {
        let us_total: Option<u64> = v.downcast_ref::<i64>().ok().map(|x| x as u64)
            .or_else(|| v.downcast_ref::<i32>().ok().map(|x| x as u64))
            .or_else(|| v.downcast_ref::<u64>().ok().map(|x| x));
        if let Some(us) = us_total { return Some(Duration::from_micros(us)); }
    }
    None
}

fn build_state_from_map(metadata: &HashMap<String, OwnedValue>, playback_status: Option<&str>, position_us: Option<i64>, playback_rate: Option<f64>) -> PlayerState {
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

async fn get_initial(conn: &Connection, bus: &str) -> zbus::Result<(String, PlayerState)> {
    let path = "/org/mpris/MediaPlayer2";
    let mp2 = Proxy::new(conn, bus, path, "org.mpris.MediaPlayer2").await?;
    let player = Proxy::new(conn, bus, path, "org.mpris.MediaPlayer2.Player").await?;

    let identity: String = mp2.get_property("Identity").await?;
    let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata").await?;
    let playback_status: Option<String> = player.get_property("PlaybackStatus").await.ok();
    // Try to get current position (in microseconds). Some players may not expose it.
    let position_us: Option<i64> = player.get_property("Position").await.ok();
    let playback_rate: Option<f64> = player.get_property("Rate").await.ok();
    let state = build_state_from_map(&metadata, playback_status.as_deref(), position_us, playback_rate);
    Ok((identity, state))
}

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<ServiceHandle> {
    let handle = spawn_service(move |mut stop| async move {
        let conn = match Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                warn!("zbus connect failed: {e}");
                return;
            }
        };
        // registry keyed by well-known name -> (id, state, last_position_us, owner_unique_name)
        let mut registry: HashMap<String, (ManagedPlayerId, PlayerState, Option<i64>, Option<String>)> = HashMap::new();

        // Initial discovery via ListNames
        if let Ok(dbus) = DBusProxy::new(&conn).await {
            if let Ok(names) = dbus.list_names().await {
                for owned in names {
                    let name = owned.to_string();
                    if !name.starts_with("org.mpris.MediaPlayer2.") { continue; }
                    if let Ok((identity, mut state)) = get_initial(&conn, &name).await {
                        if let Ok(id) = driver.register_player(format!("native-linux-mpris:{identity}.{name}")).await {
                            let _ = driver.update_player_state(id, state.clone()).await;
                            // After state, push a fresh initial timeline with accurate position if available
                            if let Some(mut tl) = state.timeline.clone() {
                                tl.update_time = SystemTime::now();
                                // fetch current playback rate and apply only if Playing
                                if let Ok(pxy) = Proxy::new(&conn, name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                    let rate: f64 = pxy.get_property("Rate").await.unwrap_or(1.0);
                                    tl.rate = if matches!(state.status, FsctStatus::Playing) { rate } else { 0.0 };
                                } else {
                                    tl.rate = if matches!(state.status, FsctStatus::Playing) { 1.0 } else { 0.0 };
                                }
                                let _ = driver.update_player_timeline(id, Some(tl.clone())).await;
                                state.timeline = Some(tl);
                            }
                            // Try get current owner unique name
                            let owner = if let Ok(db) = DBusProxy::new(&conn).await {
                                if let Ok(bus_name) = BusName::try_from(name.as_str()) {
                                    db.get_name_owner(bus_name).await.ok().map(|u| u.to_string())
                                } else { None }
                            } else { None };
                            registry.insert(name, (id, state, None, owner));
                        }
                    }
                }
            }
        }

        // Install explicit match rules via DBus AddMatch to ensure delivery of needed signals
        if let Ok(bus) = DBusProxy::new(&conn).await {
            let _ = bus.add_match_rule(MatchRule::builder()
                .interface("org.freedesktop.DBus").unwrap()
                .member("NameOwnerChanged").unwrap()
                .build()).await;
            let _ = bus.add_match_rule(MatchRule::builder()
                .interface("org.freedesktop.DBus.Properties").unwrap()
                .member("PropertiesChanged").unwrap()
                .path("/org/mpris/MediaPlayer2").unwrap()
                .build()).await;
            let _ = bus.add_match_rule(MatchRule::builder()
                .interface("org.mpris.MediaPlayer2.Player").unwrap()
                .member("Seeked").unwrap()
                .path("/org/mpris/MediaPlayer2").unwrap()
                .build()).await;
        }

        let mut stream = MessageStream::from(&conn);
        use futures_util::StreamExt;
        loop {
            tokio::select! {
                _ = stop.signaled() => break,
                Some(Ok(msg)) = stream.next() => {
                    let hdr = msg.header();
                    if hdr.member().map(|m| m.as_str()) == Some("NameOwnerChanged") {
                        if let Ok((name, old_owner, new_owner)) = msg.body().deserialize::<(String, String, String)>() {
                            if !name.starts_with("org.mpris.MediaPlayer2.") { continue; }
                            if old_owner.is_empty() && !new_owner.is_empty() {
                                let owner = Some(new_owner.clone());
                                if let Ok((identity, mut state)) = get_initial(&conn, &name).await {
                                    if let Ok(id) = driver.register_player(format!("native-linux-mpris:{identity}.{name}")).await {
                                        let _ = driver.update_player_state(id, state.clone()).await;
                                        // push initial timeline if available
                                        if let Some(mut tl) = state.timeline.clone() {
                                            tl.update_time = SystemTime::now();
                                            if let Ok(pxy) = Proxy::new(&conn, name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                                let rate: f64 = pxy.get_property("Rate").await.unwrap_or(1.0);
                                                tl.rate = if matches!(state.status, FsctStatus::Playing) { rate } else { 0.0 };
                                            } else {
                                                tl.rate = if matches!(state.status, FsctStatus::Playing) { 1.0 } else { 0.0 };
                                            }
                                            let _ = driver.update_player_timeline(id, Some(tl.clone())).await;
                                            state.timeline = Some(tl);
                                        }
                                        registry.insert(name, (id, state, None, owner));
                                    }
                                }
                            } else if !old_owner.is_empty() && new_owner.is_empty() {
                                if let Some((id, _, _, _)) = registry.remove(&name) {
                                    let _ = driver.update_player_state(id, PlayerState::default()).await;
                                    let _ = driver.unregister_player(id).await;
                                }
                            }
                        }
                    } else if hdr.member().map(|m| m.as_str()) == Some("PropertiesChanged") {
                        if let Ok((iface, changed, _inv)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() {
                            if iface != "org.mpris.MediaPlayer2.Player" { continue; }
                            // Route only to players whose owner matches the signal sender (if available)
                            let sender = hdr.sender().map(|s| s.to_string());
                            for (_name, (id, last, _pos, owner)) in registry.iter_mut() {
                                if owner.as_ref().is_some() && sender.is_some() && owner.as_ref() != sender.as_ref() { continue; }
                                let status = changed.get("PlaybackStatus").and_then(|v| v.downcast_ref::<Str>().ok().map(|s| s.to_string()))
                                    .or_else(|| changed.get("PlaybackStatus").and_then(|v| v.downcast_ref::<String>().ok().map(|s| s.clone())));
                                // If Rate changed directly, update timeline accordingly
                                if let Some(v) = changed.get("Rate") {
                                    if let Some(mut tl) = last.timeline.clone() {
                                        let mut rate_val = 1.0;
                                        if let Ok(val) = v.downcast_ref::<f64>() { rate_val = val; }
                                        else if let Ok(iv) = v.downcast_ref::<i64>() { rate_val = iv as f64; }
                                        else if let Ok(iv) = v.downcast_ref::<i32>() { rate_val = iv as f64; }
                                        tl.rate = if matches!(last.status, FsctStatus::Playing) { rate_val } else { 0.0 };
                                        tl.update_time = SystemTime::now();
                                        // also refresh duration, as it might change along with rate updates
                                        if let Ok(pxy) = Proxy::new(&conn, _name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                            if let Ok(meta) = pxy.get_property::<HashMap<String, OwnedValue>>("Metadata").await {
                                                if let Some(new_dur) = extract_duration_from_metadata(&meta) {
                                                    if new_dur != tl.duration { tl.duration = new_dur; }
                                                }
                                            }
                                        }
                                        let _ = driver.update_player_timeline(*id, Some(tl.clone())).await;
                                        last.timeline = Some(tl);
                                    }
                                }
                                let mut new_state = last.clone();
                                if let Some(s) = status.as_deref() { new_state.status = map_status(Some(s)); }
                                // If "Metadata" present, refresh via proxy to avoid complex Value downcasts
                                if changed.contains_key("Metadata") {
                                    // find the well-known name for this owner
                                    // Since we’re in the loop, _name is the well-known name
                                    let well_known = _name.clone();
                                    if let Ok(pxy) = Proxy::new(&conn, well_known.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                        let meta: Option<HashMap<String, OwnedValue>> = pxy.get_property("Metadata").await.ok();
                                        let status_str: Option<String> = pxy.get_property("PlaybackStatus").await.ok();
                                        let rate_opt: Option<f64> = pxy.get_property("Rate").await.ok();
                                        if let Some(meta_map) = meta.as_ref() {
                                            let prev_tl = last.timeline.clone();
                                            new_state = build_state_from_map(meta_map, status_str.as_deref(), None, rate_opt);
                                            // On any Metadata change, rebuild timeline from fresh Position; do not carry over old position
                                            if let (Some(_old_tl), Some(mut new_tl)) = (prev_tl, new_state.timeline.clone()) {
                                                // Fetch fresh position (microseconds) if available
                                                let mut new_pos = Duration::from_secs(0);
                                                if let Ok(pxy) = Proxy::new(&conn, well_known.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                                    if let Ok(pos_us) = pxy.get_property::<i64>("Position").await {
                                                        new_pos = Duration::from_micros(pos_us as u64);
                                                    }
                                                }
                                                // Clamp to duration if needed
                                                if new_pos > new_tl.duration {
                                                    new_pos = new_tl.duration;
                                                }
                                                new_tl.position = new_pos;
                                                new_tl.update_time = SystemTime::now();
                                                let _ = driver.update_player_timeline(*id, Some(new_tl.clone())).await;
                                                new_state.timeline = Some(new_tl);
                                            }
                                        }
                                    }
                                }
                                if *last != new_state {
                                    let prev = last.clone();
                                    *last = new_state.clone();
                                    let _ = driver.update_player_state(*id, new_state.clone()).await;
                                    // On any PlaybackStatus change, push fresh timeline with current position and appropriate rate
                                    if prev.status != last.status {
                                        if let Some(mut tl) = last.timeline.clone().or(prev.timeline.clone()) {
                                            // Try to get Position property for accurate value
                                            if let Ok(pxy) = Proxy::new(&conn, _name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                                if let Ok(pos_us) = pxy.get_property::<i64>("Position").await {
                                                    tl.position = Duration::from_micros(pos_us as u64);
                                                }
                                            }
                                            // Determine playback rate from PropertiesChanged or fallback to proxy
                                            let mut rate_val: f64 = 1.0;
                                            // try read Rate from changed map directly
                                            if let Some(v) = changed.get("Rate") {
                                                if let Ok(val) = v.downcast_ref::<f64>() { rate_val = val; }
                                                else if let Ok(iv) = v.downcast_ref::<i64>() { rate_val = iv as f64; }
                                                else if let Ok(iv) = v.downcast_ref::<i32>() { rate_val = iv as f64; }
                                            } else if let Ok(pxy) = Proxy::new(&conn, _name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                                rate_val = pxy.get_property::<f64>("Rate").await.unwrap_or(1.0);
                                            }
                                            tl.rate = if matches!(last.status, FsctStatus::Playing) { rate_val } else { 0.0 };
                                            tl.update_time = SystemTime::now();
                                            // also refresh duration, as it might change when status/rate changes
                                            if let Ok(pxy2) = Proxy::new(&conn, _name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                                if let Ok(meta) = pxy2.get_property::<HashMap<String, OwnedValue>>("Metadata").await {
                                                    if let Some(new_dur) = extract_duration_from_metadata(&meta) {
                                                        if new_dur != tl.duration { tl.duration = new_dur; }
                                                    }
                                                }
                                            }
                                            let _ = driver.update_player_timeline(*id, Some(tl.clone())).await;
                                            // keep internal state timeline in sync after status change
                                            last.timeline = Some(tl);
                                        }
                                    }
                                }
                            }
                        }
                    } else if hdr.member().map(|m| m.as_str()) == Some("Seeked") {
                        if let Ok((pos_us,)) = msg.body().deserialize::<(i64,)>() {
                            let sender = hdr.sender().map(|s| s.to_string());
                            for (_name, (id, last, pos_slot, owner)) in registry.iter_mut() {
                                if owner.as_ref().is_some() && sender.as_ref().is_some() && owner.as_ref() != sender.as_ref() { continue; }
                                *pos_slot = Some(pos_us);
                                if let Some(mut tl) = last.timeline.clone() {
                                    tl.position = Duration::from_micros(pos_us as u64);
                                    tl.update_time = SystemTime::now();
                                    // also refresh duration on position updates, since it may change
                                    if let Ok(pxy) = Proxy::new(&conn, _name.as_str(), "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await {
                                        if let Ok(meta) = pxy.get_property::<HashMap<String, OwnedValue>>("Metadata").await {
                                            if let Some(new_dur) = extract_duration_from_metadata(&meta) {
                                                if new_dur != tl.duration { tl.duration = new_dur; }
                                            }
                                        }
                                    }
                                    let _ = driver.update_player_timeline(*id, Some(tl.clone())).await;
                                    // keep last state timeline in sync
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
