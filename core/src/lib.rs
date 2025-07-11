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
pub mod player;

mod player_watch;
mod service_entry;
mod devices_watch;
mod service_state;

pub use service_entry::run_service;
pub use player_watch::run_player_watch;
pub use devices_watch::run_devices_watch;
pub use devices_watch::DevicesWatchHandle;
pub use player::Player;
pub use player_watch::NoopPlayerEventListener;

pub use nusb::DeviceId;
pub use devices_watch::DeviceMap;
pub use devices_watch::DevicesPlayerEventApplier;
pub use service_state::FsctServiceState;
