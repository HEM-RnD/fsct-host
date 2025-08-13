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
pub mod usb;
pub mod definitions;

#[deprecated]
pub mod player;

#[deprecated]
mod player_watch;

mod player_manager;
pub mod player_state_applier;
pub mod player_events;
pub mod orchestrator;
mod service_entry;
pub mod service;
pub mod driver;

#[deprecated]
mod devices_watch;
pub mod device_manager;
pub mod usb_device_watch;
pub mod player_state;
mod service_state;
mod device_uuid_calculator;

pub use service_entry::run_service;
pub use player_watch::run_player_watch;
pub use devices_watch::run_devices_watch;
pub use devices_watch::DevicesWatchHandle;
pub use devices_watch::DevicesPlayerEventApplier;
pub use player::Player;
pub use player_watch::NoopPlayerEventListener;

pub use player_manager::{ManagedPlayerId, PlayerManager};
pub use player_state::PlayerState;
pub use player_events::PlayerEvent;
pub use orchestrator::Orchestrator;

// Export driver abstraction
pub use driver::{FsctDriver, LocalDriver};

// Export device management types
pub use device_manager::{DeviceManager, DeviceManagement, DeviceControl, ManagedDeviceId, DeviceEvent, DeviceManagerError};
pub use usb_device_watch::run_usb_device_watch;
pub use service::{ServiceHandle, StopHandle, spawn_service, MultiServiceHandle};

pub use nusb::DeviceId;
pub use service_state::FsctServiceState;