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

/// Returns the default path of the Unix Domain Socket used by FSCT IPC on Linux.
#[cfg(target_os = "linux")]
pub fn default_endpoint_path() -> &'static str {
    "/run/fsct/fsct.sock"
}

/// Returns the default path of the Named Pipe used by FSCT IPC on Windows.
#[cfg(target_os = "windows")]
pub fn default_endpoint_path() -> &'static str {
    "\\\\.\\pipe\\fsct_driver"
}

/// Returns the default path of the Unix Domain Socket used by FSCT IPC on macOS.
#[cfg(target_os = "macos")]
pub fn default_endpoint_path() -> &'static str {
    "/var/run/fsct/fsct.sock"
}
