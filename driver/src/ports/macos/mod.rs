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

use std::os::fd::{IntoRawFd, OwnedFd};

pub mod player;

/// Returns the file descriptor provided by launchd socket activation (if any).
///
/// This expects a socket named "fsct-socket" in the launchd .plist under the `Sockets` key.
/// If the service wasn't activated via launchd sockets, returns None.
pub fn get_socket_activation_fd() -> Option<OwnedFd> {
    use std::os::fd::FromRawFd;

    let fds: Vec<_> = raunch::activate_socket("fsct-socket")
        .ok()?
        // map to OwnedFd and collect, so all discarded fds are closed automatically
        .into_iter()
        .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
        .collect();
    // return the first fd, discarding the rest
    fds.into_iter().next()
}
