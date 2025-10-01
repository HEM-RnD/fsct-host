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
mod service_main;
mod ports;
pub mod ipc;
pub mod usb;
pub mod player_manager;
pub mod player_state_applier;
pub mod player_events;
pub mod orchestrator;
pub mod joinable_task;
pub mod device_manager;
pub mod usb_device_watch;
pub mod inprocess_driver;

pub use ports::*;

pub use service_main::StopSignal;
pub use service_main::ServiceStateListener;
pub use service_main::ServiceStateNullListener;

pub use service::fsct_main;
pub use player::run_os_watcher;
pub use ipc::IpcServer;


// Export device management types
pub use device_manager::{DeviceControl, DeviceEvent, DeviceManagement, DeviceManager, DeviceManagerError};
pub use usb_device_watch::run_usb_device_watch;
pub use joinable_task::{spawn_service, JoinableTaskHandle, MultiJoinableTaskHandle, StopHandle};

pub use player_events::PlayerEvent;
pub use orchestrator::Orchestrator;
// Export driver abstraction
pub use inprocess_driver::LocalDriver;

pub use nusb::DeviceId;
