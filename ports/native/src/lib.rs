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

use fsct_core::player::Player;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

#[allow(unreachable_code)]

pub async fn initialize_native_platform_player() -> anyhow::Result<Player> {
    #[cfg(target_os = "windows")]
    {
        let windows_player = windows::WindowsPlatformGlobalSessionManager::new()
            .await?;
        return Ok(Player::new(windows_player));
    }
    #[cfg(target_os = "macos")]
    {
        return Ok(Player::new(
            macos::MacOSPlaybackManager::new()?
        ));
    }
    {
        panic!("Unsupported platform");
    }
}
