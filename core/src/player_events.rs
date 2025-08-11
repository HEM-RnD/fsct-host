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

use crate::device_manager::ManagedDeviceId;
use crate::player_state::PlayerState;
use crate::player_manager::ManagedPlayerId;

/// Events emitted by PlayerManager about player lifecycle, assignments and state changes.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// A new player has been registered.
    Registered { player_id: ManagedPlayerId, self_id: String },

    /// A player has been unregistered.
    Unregistered { player_id: ManagedPlayerId },

    /// A player has been assigned to a specific device.
    Assigned { player_id: ManagedPlayerId, device_id: ManagedDeviceId },

    /// A player has been unassigned from a specific device.
    Unassigned { player_id: ManagedPlayerId, device_id: ManagedDeviceId },

    /// Player's state has been updated. Consumers decide where to propagate it.
    StateUpdated { player_id: ManagedPlayerId, state: PlayerState },

    /// Preferred player selection changed. Contains the new preferred player id or None.
    PreferredChanged { preferred: Option<ManagedPlayerId> },
}
