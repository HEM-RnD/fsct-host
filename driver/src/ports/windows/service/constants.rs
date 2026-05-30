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

// Service constants
pub const DRIVER_SERVICE_NAME: &str = "FsctDriverService";
pub const DRIVER_SERVICE_DISPLAY_NAME: &str = "Ferrum Streaming Control Technology Driver Service";
pub const DRIVER_SERVICE_DESCRIPTION: &str = "This service provides support for Ferrum Streaming Control Technology with compatible devices. It acts as user space driver and uses WINUSB as kernel space driver.";

pub const USER_SERVICE_NAME: &str = "FsctUserService";
pub const USER_SERVICE_DISPLAY_NAME: &str = "Ferrum Streaming Control Technology User Service";
pub const USER_SERVICE_DESCRIPTION: &str = "This service gets track metadata from players registered in user \
session and send them into FSCT driver via local pipe.";
