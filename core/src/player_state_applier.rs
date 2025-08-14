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
use std::sync::{Arc, Mutex};

use anyhow::Error;
use std::future::Future;
use std::pin::Pin;

use crate::device_manager::{DeviceControl, ManagedDeviceId};
use crate::player_state::PlayerState;
use crate::definitions::{FsctStatus, FsctTextMetadata, TimelineInfo};

/// Abstraction for applying PlayerState to devices.
///
/// This separates device-setting logic from PlayerManager. Implementations may:
/// - call device APIs directly (synchronous .await), or
/// - enqueue commands and process them in background tasks (recommended for isolation/backpressure).
pub trait PlayerStateApplier: Send + Sync {
    /// Apply the given player state to a specific device.
    fn apply_to_device<'a>(&'a self, device_id: ManagedDeviceId, state: &'a PlayerState)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

    /// Apply only status independently.
    fn apply_status<'a>(&'a self, device_id: ManagedDeviceId, status: FsctStatus)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

    /// Apply only timeline/progress independently.
    fn apply_timeline<'a>(&'a self, device_id: ManagedDeviceId, timeline: Option<TimelineInfo>)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

    /// Apply a single text field independently.
    fn apply_text<'a>(&'a self, device_id: ManagedDeviceId, text_id: FsctTextMetadata, text: Option<&'a str>)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;
}

/// Direct implementation that wraps a DeviceControl provider.
/// Keeps behavior identical to previous PlayerManager logic while decoupling responsibilities.
pub struct DirectDeviceControlApplier<T: DeviceControl + Send + Sync + 'static> {
    device_control: Arc<T>,
    last_applied: Mutex<HashMap<ManagedDeviceId, PlayerState>>, // per-device snapshot to diff against
}

impl<T: DeviceControl + Send + Sync + 'static> DirectDeviceControlApplier<T> {
    pub fn new(device_control: Arc<T>) -> Self {
        Self {
            device_control,
            last_applied: Mutex::new(HashMap::new()),
        }
    }
}

impl<T: DeviceControl + Send + Sync + 'static> PlayerStateApplier for DirectDeviceControlApplier<T> {
    fn apply_to_device<'a>(&'a self, device_id: ManagedDeviceId, state: &'a PlayerState)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            // todo consider better error handling
            
            // in multitasking, this could be a race condition if the device is set to a different state in the meantime
            // but then it would be better to implement queue-based applier instead
            // then applying task would be only one that changes the state
            // so it would be write it the same way as here

            // Take a snapshot of the previous state for this device without holding the lock across awaits.
            let prev_state = {
                let guard = self
                    .last_applied
                    .lock()
                    .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;
                guard.get(&device_id).cloned()
            };

            // Decide what changed
            let status_changed = prev_state
                .as_ref()
                .map(|p| p.status != state.status)
                .unwrap_or(true);

            let progress_changed = prev_state
                .as_ref()
                .map(|p| p.timeline != state.timeline)
                .unwrap_or(true);

            // Collect text changes (covers both set and clear)
            let mut text_changes: Vec<(crate::definitions::FsctTextMetadata, Option<&str>)> = Vec::new();
            for text_id in state.texts.iter_id() {
                let new_val = state.texts.get_text(*text_id);
                let changed = match prev_state.as_ref() {
                    Some(prev) => prev.texts.get_text(*text_id) != new_val,
                    None => new_val.is_some(),
                };
                if changed {
                    text_changes.push((*text_id, new_val.as_deref()));
                }
            }

            // Apply only the changed parts
            if status_changed {
                self.device_control
                    .set_status(device_id, state.status)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to set status: {}", e))?;
            }

            if progress_changed {
                self.device_control
                    .set_progress(device_id, state.timeline.clone())
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to set progress: {}", e))?;
            }

            for (text_id, new_val) in text_changes {
                if let Err(e) = self
                    .device_control
                    .set_current_text(device_id, text_id, new_val)
                    .await
                {
                    // Fail-fast to keep behavior consistent
                    return Err(anyhow::anyhow!("Failed to set text: {}", e));
                }
            }

            // Update snapshot
            {
                let mut guard = self
                    .last_applied
                    .lock()
                    .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;
                guard.insert(device_id, state.clone());
            }

            Ok(())
        })
    }

    fn apply_status<'a>(&'a self, device_id: ManagedDeviceId, status: FsctStatus)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            // Snapshot previous status (no await while locked)
            let unchanged = {
                let guard = self
                    .last_applied
                    .lock()
                    .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;
                let player_state = guard
                    .get(&device_id)
                    .ok_or_else(|| anyhow::anyhow!("PlayerStateApplier: device not found"))?;
                player_state.status == status
            };

            if unchanged {
                return Ok(())
            }

            // Apply
            self.device_control
                .set_status(device_id, status)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set status: {}", e))?;

            // Update only status in snapshot
            let mut guard = self
                .last_applied
                .lock()
                .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;
            let entry = guard.entry(device_id).or_insert_with(PlayerState::default);
            entry.status = status;
            Ok(())
        })
    }

    fn apply_timeline<'a>(&'a self, device_id: ManagedDeviceId, timeline: Option<TimelineInfo>)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            // Snapshot previous timeline
            let unchanged = {
                let guard = self
                    .last_applied
                    .lock()
                    .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;

                let player_state = guard
                    .get(&device_id)
                    .ok_or_else(|| anyhow::anyhow!("PlayerStateApplier: device not found"))?;
                player_state.timeline == timeline
            };

            // If unchanged (and we have a previous state), skip
            if unchanged {
                return Ok(());
            }

            // Apply
            self.device_control
                .set_progress(device_id, timeline.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set progress: {}", e))?;

            // Update only timeline in snapshot
            let mut guard = self
                .last_applied
                .lock()
                .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;
            let entry = guard.entry(device_id).or_insert_with(PlayerState::default);
            entry.timeline = timeline;
            Ok(())
        })
    }

    fn apply_text<'a>(&'a self, device_id: ManagedDeviceId, text_id: FsctTextMetadata, text: Option<&'a str>)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            // Snapshot previous text
            let unchanged: bool = {
                let guard = self
                    .last_applied
                    .lock()
                    .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;
                let player_state = guard
                    .get(&device_id)
                    .ok_or_else(|| anyhow::anyhow!("PlayerStateApplier: device not found"))?;
                player_state.texts.get_text(text_id).as_ref().map(|s|s.as_str()) == text
            };

            if unchanged {
                return Ok(());
            }

            // Apply
            self.device_control
                .set_current_text(device_id, text_id, text)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set text: {}", e))?;

            // Update only the specific text in snapshot
            let mut guard = self
                .last_applied
                .lock()
                .map_err(|_| anyhow::anyhow!("PlayerStateApplier lock poisoned"))?;
            let entry = guard.entry(device_id).or_insert_with(PlayerState::default);
            let target = entry.texts.get_mut_text(text_id);
            *target = text.map(|s| s.to_string());
            Ok(())
        })
    }
}

// Sketch: An alternative async queue-based applier could look like this (not used by default):
// - It owns an mpsc::Sender<Command> and spawns a worker task that processes commands.
// - PlayerManager would only enqueue (non-blocking) and return.
// This allows isolating device IO and applying backpressure. Left out for minimal code changes.
