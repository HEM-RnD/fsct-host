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

use log::{info, warn};
use std::os::fd::{FromRawFd, OwnedFd};

pub mod player;

fn is_triggered_by_systemd_socket_activation() -> bool {
    let listen_fds = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let listen_pid = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let this_process_pid = std::process::id();
    // Be sure that FDs are assigned to the correct (this) process; otherwise systemd will not pass them to us.
    listen_fds > 0 && listen_pid == this_process_pid
}

pub fn get_socket_activation_fd() -> Option<OwnedFd> {
    let systemd_socket_activated = is_triggered_by_systemd_socket_activation();

    if systemd_socket_activated {
        info!("systemd socket activation detected, using fd 3");
        // If systemd socket activation is used, use the pre-opened listening socket (fd=3) passed by systemd/socket-activation helper.
        // 3 is the only fd that systemd/socket-activation helper will pass to the process,
        // and it has to be valid fd for the process to be able to use it.
        let fd = unsafe { OwnedFd::from_raw_fd(3) };
        Some(fd)
    } else {
        info!(
            "systemd socket activation not detected, using {}",
            fsct::default_endpoint_path()
        );
        None
    }
}
