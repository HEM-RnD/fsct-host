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

mod cli;
pub mod device_manager;
pub mod inprocess_driver;
pub mod ipc;
pub mod joinable_task;
pub mod orchestrator;
pub mod player_events;
pub mod player_manager;
pub mod player_state_applier;
mod ports;
mod service_main;
pub mod usb;
pub mod usb_device_watch;

pub use ports::*;

pub use service_main::ServiceStateListener;
pub use service_main::ServiceStateNullListener;
pub use service_main::StopSignal;

pub use ipc::IpcServer;
pub use player::run_os_watcher;
pub use service::fsct_main;

// Export device management types
pub use device_manager::{DeviceControl, DeviceEvent, DeviceManagement, DeviceManager, DeviceManagerError};
pub use joinable_task::{JoinableTaskHandle, MultiJoinableTaskHandle, StopHandle, spawn_service};
pub use usb_device_watch::run_usb_device_watch;

pub use orchestrator::Orchestrator;
pub use player_events::PlayerEvent;
// Export driver abstraction
pub use inprocess_driver::LocalDriver;

pub use nusb::DeviceId;
