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

use std::sync::Arc;

use anyhow::Error;
use std::future::Future;
use std::pin::Pin;

use crate::device_manager::{DeviceControl, ManagedDeviceId};
use crate::player_state::PlayerState;

/// Abstraction for applying PlayerState to devices.
///
/// This separates device-setting logic from PlayerManager. Implementations may:
/// - call device APIs directly (synchronous .await), or
/// - enqueue commands and process them in background tasks (recommended for isolation/backpressure).
pub trait PlayerStateApplier: Send + Sync {
    /// Apply the given player state to a specific device.
    fn apply_to_device<'a>(&'a self, device_id: ManagedDeviceId, state: &'a PlayerState)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

    /// Apply the currently active unassigned player state to all devices that do not
    /// have a player assigned. The strategy for choosing the active unassigned player
    /// lives above this layer; this method only propagates a given state to devices.
    ///
    /// For now, this is a no-op default in the direct implementation and can be
    /// implemented by a higher-level component that knows the set of unassigned devices.
    fn apply_to_unassigned_devices<'a>(&'a self, _state: &'a PlayerState)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// Direct implementation that wraps a DeviceControl provider.
/// Keeps behavior identical to previous PlayerManager logic while decoupling responsibilities.
pub struct DirectDeviceControlApplier<T: DeviceControl + Send + Sync + 'static> {
    device_control: Arc<T>,
}

impl<T: DeviceControl + Send + Sync + 'static> DirectDeviceControlApplier<T> {
    pub fn new(device_control: Arc<T>) -> Self {
        Self { device_control }
    }
}

impl<T: DeviceControl + Send + Sync + 'static> PlayerStateApplier for DirectDeviceControlApplier<T> {
    fn apply_to_device<'a>(&'a self, device_id: ManagedDeviceId, state: &'a PlayerState)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            // Apply status
            self.device_control
                .set_status(device_id, state.status)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set status: {}", e))?;

            // Apply timeline
            self.device_control
                .set_progress(device_id, state.timeline.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set progress: {}", e))?;

            // Apply texts
            for (text_id, text) in state.texts.iter() {
                if let Err(e) = self
                    .device_control
                    .set_current_text(device_id, text_id, text.as_deref())
                    .await
                {
                    // Fail-fast behavior kept to match previous logic.
                    return Err(anyhow::anyhow!("Failed to set text: {}", e));
                }
            }

            Ok(())
        })
    }

    fn apply_to_unassigned_devices<'a>(&'a self, _state: &'a PlayerState)
        -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        // In the direct variant we do nothing by default because we don't have
        // a device inventory here. A higher-level orchestrator should call
        // apply_to_device for each unassigned device as needed.
        Box::pin(async { Ok(()) })
    }
}

// Sketch: An alternative async queue-based applier could look like this (not used by default):
// - It owns an mpsc::Sender<Command> and spawns a worker task that processes commands.
// - PlayerManager would only enqueue (non-blocking) and return.
// This allows isolating device IO and applying backpressure. Left out for minimal code changes.
