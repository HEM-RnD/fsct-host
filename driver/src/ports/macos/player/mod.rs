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

use fsct::definitions::{FsctStatus, ManagedPlayerId, TimelineInfo};
use fsct::player_state::{PlayerState, TrackMetadata};
use fsct::FsctDriver;
use crate::joinable_task::{spawn_service, JoinableTaskHandle};
use media_remote::{NowPlaying, NowPlayingInfo, NowPlayingJXA, Subscription};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use anyhow::anyhow;
use tokio::sync::mpsc;
use tokio::time;

#[allow(dead_code)]
struct NowPlayingWrapper {
    now_playing: NowPlaying,
}

unsafe impl Send for NowPlayingWrapper {}

#[derive(Clone)]
struct ScriptNowPlayingInfo {
    title: String,
    artist: String,
    album: String,
    elapsed_time: f64,
    duration: f64,
    is_playing: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveSource {
    Music,
    Spotify,
}


fn get_current_track(now_playing_info: &NowPlayingInfo) -> TrackMetadata {
    let mut texts = TrackMetadata::default();
    texts.title = now_playing_info.title.clone();
    texts.artist = now_playing_info.artist.clone();
    texts.album = now_playing_info.album.clone();
    texts.genre = None;

    texts
}

fn parse_applescript_float(value: &str) -> Option<f64> {
    value.trim().replace(',', ".").parse::<f64>().ok()
}

fn normalize_duration_seconds(duration_value: f64) -> f64 {
    if duration_value > 10000.0 {
        duration_value / 1000.0
    } else {
        duration_value
    }
}

fn script_info_to_state(info: &ScriptNowPlayingInfo) -> PlayerState {
    PlayerState {
        status: if info.is_playing { FsctStatus::Playing } else { FsctStatus::Paused },
        texts: TrackMetadata {
            title: Some(info.title.clone()),
            artist: Some(info.artist.clone()),
            album: Some(info.album.clone()),
            genre: None,
        },
        timeline: Some(TimelineInfo {
            position: Duration::from_secs_f64(info.elapsed_time.max(0.0)),
            update_time: SystemTime::now(),
            duration: Duration::from_secs_f64(info.duration.max(0.0)),
            rate: if info.is_playing { 1.0 } else { 0.0 },
        }),
    }
}

fn music_now_playing_via_applescript(allow_paused: bool) -> Option<ScriptNowPlayingInfo> {
    let script = if allow_paused {
        r#"
if application id "com.apple.Music" is running then
    tell application id "com.apple.Music"
        set currentState to player state
        if currentState is playing or currentState is paused then
            set trackName to (name of current track)
            set trackArtist to (artist of current track)
            set trackAlbum to (album of current track)
            set trackPosition to (player position)
            set trackDuration to (duration of current track)
            return trackName & "|||FSCT|||" & trackArtist & "|||FSCT|||" & trackAlbum & "|||FSCT|||" & (trackPosition as string) & "|||FSCT|||" & (trackDuration as string) & "|||FSCT|||" & (currentState as string)
        else
            return ""
        end if
    end tell
else
    return ""
end if
"#
    } else {
        r#"
if application id "com.apple.Music" is running then
    tell application id "com.apple.Music"
        set currentState to player state
        if currentState is playing then
            set trackName to (name of current track)
            set trackArtist to (artist of current track)
            set trackAlbum to (album of current track)
            set trackPosition to (player position)
            set trackDuration to (duration of current track)
            return trackName & "|||FSCT|||" & trackArtist & "|||FSCT|||" & trackAlbum & "|||FSCT|||" & (trackPosition as string) & "|||FSCT|||" & (trackDuration as string) & "|||FSCT|||" & (currentState as string)
        else
            return ""
        end if
    end tell
else
    return ""
end if
"#
    };

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parts: Vec<&str> = trimmed.splitn(6, "|||FSCT|||").collect();
    if parts.len() != 6 {
        return None;
    }

    let elapsed_time = parse_applescript_float(parts[3])?;
    let duration = parse_applescript_float(parts[4])?;
    let is_playing = matches!(parts[5].trim(), "playing");

    Some(ScriptNowPlayingInfo {
        title: parts[0].to_string(),
        artist: parts[1].to_string(),
        album: parts[2].to_string(),
        elapsed_time,
        duration: normalize_duration_seconds(duration),
        is_playing,
    })
}

fn spotify_now_playing_via_applescript(allow_paused: bool) -> Option<ScriptNowPlayingInfo> {
    let script = if allow_paused {
        r#"
if application id "com.spotify.client" is running then
    tell application id "com.spotify.client"
        set currentState to player state
        if currentState is playing or currentState is paused then
            set trackName to (name of current track)
            set trackArtist to (artist of current track)
            set trackAlbum to (album of current track)
            set trackPosition to (player position)
            set trackDuration to (duration of current track)
            return trackName & "|||FSCT|||" & trackArtist & "|||FSCT|||" & trackAlbum & "|||FSCT|||" & (trackPosition as string) & "|||FSCT|||" & (trackDuration as string) & "|||FSCT|||" & (currentState as string)
        else
            return ""
        end if
    end tell
else
    return ""
end if
"#
    } else {
        r#"
if application id "com.spotify.client" is running then
    tell application id "com.spotify.client"
        set currentState to player state
        if currentState is playing then
            set trackName to (name of current track)
            set trackArtist to (artist of current track)
            set trackAlbum to (album of current track)
            set trackPosition to (player position)
            set trackDuration to (duration of current track)
            return trackName & "|||FSCT|||" & trackArtist & "|||FSCT|||" & trackAlbum & "|||FSCT|||" & (trackPosition as string) & "|||FSCT|||" & (trackDuration as string) & "|||FSCT|||" & (currentState as string)
        else
            return ""
        end if
    end tell
else
    return ""
end if
"#
    };

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parts: Vec<&str> = trimmed.splitn(6, "|||FSCT|||").collect();
    if parts.len() != 6 {
        return None;
    }

    let elapsed_time = parse_applescript_float(parts[3])?;
    let duration = parse_applescript_float(parts[4])?;
    let is_playing = matches!(parts[5].trim(), "playing");

    Some(ScriptNowPlayingInfo {
        title: parts[0].to_string(),
        artist: parts[1].to_string(),
        album: parts[2].to_string(),
        elapsed_time,
        duration: normalize_duration_seconds(duration),
        is_playing,
    })
}

fn get_timeline_info(now_playing_info: &NowPlayingInfo) -> Option<TimelineInfo> {
    let duration = now_playing_info.duration?;
    let position = now_playing_info.elapsed_time.unwrap_or(0.0);
    let update_time = now_playing_info.info_update_time.unwrap_or(SystemTime::now());
    let is_playing = now_playing_info.is_playing.unwrap_or(false);
    let rate = if is_playing {
        now_playing_info.playback_rate.unwrap_or(0.0)
    } else {
        0.0
    };

    Some(TimelineInfo {
        position: Duration::from_secs_f64(position),
        update_time,
        duration: Duration::from_secs_f64(duration),
        rate: rate as f64,
    })
}

fn get_status(now_playing_info: &NowPlayingInfo) -> FsctStatus {
    match now_playing_info.playback_rate {
        Some(0.0) => FsctStatus::Paused,
        Some(_) => FsctStatus::Playing,
        None => FsctStatus::Stopped,
    }
}

fn build_state(info: &NowPlayingInfo) -> PlayerState {
    PlayerState {
        status: get_status(info),
        texts: get_current_track(info),
        timeline: get_timeline_info(info),
    }
}

async fn push_state(
    driver: Arc<dyn FsctDriver>,
    player_id: ManagedPlayerId,
    previous_state: &mut PlayerState,
    previous_source: &mut Option<ActiveSource>,
    info: Option<NowPlayingInfo>,
) {
    let spotify_info = spotify_now_playing_via_applescript(true);
    let music_info = music_now_playing_via_applescript(true);

    if let Some(spotify) = spotify_info.as_ref() && spotify.is_playing {
        *previous_source = Some(ActiveSource::Spotify);
        let state = script_info_to_state(spotify);
        if *previous_state != state {
            *previous_state = state.clone();
            let _ = driver.update_player_state(player_id, state).await;
        }
        return;
    }

    if let Some(music) = music_info.as_ref() && music.is_playing {
        *previous_source = Some(ActiveSource::Music);
        let state = script_info_to_state(music);
        if *previous_state != state {
            *previous_state = state.clone();
            let _ = driver.update_player_state(player_id, state).await;
        }
        return;
    }

    let info_ref = info.as_ref();

    if *previous_source == Some(ActiveSource::Spotify)
        && let Some(spotify) = spotify_info.as_ref()
    {
        let opposite_is_playing = info_ref
            .map(|value| {
                value.bundle_id.as_deref() == Some("com.apple.Music")
                    && get_status(value) == FsctStatus::Playing
            })
            .unwrap_or(false);

        if !opposite_is_playing {
            let state = script_info_to_state(spotify);
            if *previous_state != state {
                *previous_state = state.clone();
                let _ = driver.update_player_state(player_id, state).await;
            }
            return;
        }
    }

    if *previous_source == Some(ActiveSource::Music)
        && let Some(music) = music_info.as_ref()
    {
        let opposite_is_playing = info_ref
            .map(|value| {
                value.bundle_id.as_deref() == Some("com.spotify.client")
                    && get_status(value) == FsctStatus::Playing
            })
            .unwrap_or(false);

        if !opposite_is_playing {
            let state = script_info_to_state(music);
            if *previous_state != state {
                *previous_state = state.clone();
                let _ = driver.update_player_state(player_id, state).await;
            }
            return;
        }
    }

    if let Some(info) = info {
        let state = build_state(&info);
        let bundle_id = info.bundle_id.as_deref();
        if bundle_id == Some("com.spotify.client") {
            *previous_source = Some(ActiveSource::Spotify);
        } else if bundle_id == Some("com.apple.Music") {
            *previous_source = Some(ActiveSource::Music);
        }
        if *previous_state != state {
            *previous_state = state.clone();
            let _ = driver.update_player_state(player_id, state).await;
        }
        return;
    }

    let state = PlayerState::default();
    *previous_source = None;
    if *previous_state != state {
        *previous_state = state.clone();
        let _ = driver.update_player_state(player_id, state).await;
    }
}

fn get_macos_version() -> Option<(u32, u32)> {
    let output = Command::new("sw_vers").arg("-productVersion").output().ok()?;

    let version_str = String::from_utf8(output.stdout).ok()?;
    let version_parts: Vec<&str> = version_str.trim().split('.').collect();

    if version_parts.len() >= 2 {
        let major = version_parts[0].parse::<u32>().ok()?;
        let minor = version_parts[1].parse::<u32>().ok()?;
        Some((major, minor))
    } else {
        None
    }
}

#[allow(dead_code)]
enum NowPlayingImpl {
    JXA(NowPlayingJXA),
    Native(NowPlayingWrapper),
}

pub async fn run_os_watcher(driver: Arc<dyn FsctDriver>) -> anyhow::Result<JoinableTaskHandle> {
    // Register a single native macOS player (for the OS global now playing)
    let player_id = driver
        .register_player("native-macos-nowplaying".to_string())
        .await
        .map_err(|e| anyhow!(e))?;

    // Spawn a single service task that consumes the queue and updates state
    let handle = spawn_service(move |mut stop| async move {
        // Channel to move updates from callback context to our service task
        let (tx, mut rx) = mpsc::unbounded_channel::<Option<NowPlayingInfo>>();

        // Choose implementation based on macOS version and set up subscriptions
        let now_playing: NowPlayingImpl = if let Some((major, minor)) = get_macos_version() && (major > 15 || (major == 15 && minor >= 4)) {
                let now_playing = NowPlayingJXA::new(Duration::from_millis(500));
                let tx_clone = tx.clone();
                now_playing.subscribe(move |guard| {
                    let _ = tx_clone.send(guard.as_ref().cloned());
                });
                // push initial state via the same queue
                let initial = now_playing.get_info().as_ref().cloned();
                let _ = tx.send(initial);

                NowPlayingImpl::JXA(now_playing)
        } else {
            // Fallback to native implementation
            let now_playing = NowPlaying::new();
            let tx_clone = tx.clone();
            now_playing.subscribe(move |guard| {
                let _ = tx_clone.send(guard.as_ref().cloned());
            });
            // push initial state via the same queue
            let initial = now_playing.get_info().as_ref().cloned();
            let _ = tx.send(initial);

            NowPlayingImpl::Native(NowPlayingWrapper { now_playing })
        };

        let mut previous_state = PlayerState::default();
        let mut previous_source: Option<ActiveSource> = None;
        let mut poll_interval = time::interval(Duration::from_secs(1));
        poll_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = stop.signaled() => {
                    break;
                }
                _ = poll_interval.tick() => {
                    let polled = match &now_playing {
                        NowPlayingImpl::JXA(value) => value.get_info().as_ref().cloned(),
                        NowPlayingImpl::Native(value) => value.now_playing.get_info().as_ref().cloned(),
                    };
                    push_state(driver.clone(), player_id, &mut previous_state, &mut previous_source, polled).await;
                }
                maybe = rx.recv() => {
                    match maybe {
                        Some(opt) => {
                            push_state(driver.clone(), player_id, &mut previous_state, &mut previous_source, opt).await;
                        }
                        None => {
                            // Sender dropped; exit loop
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(handle)
}
