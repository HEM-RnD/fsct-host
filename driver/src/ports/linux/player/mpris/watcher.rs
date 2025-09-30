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

use futures_util::Stream;
use zbus::fdo::DBusProxy;
use zbus::names::OwnedBusName;
use crate::ports::linux::player::mpris::player::Player;

pub struct SessionWatcher {
    conn: zbus::Connection,
}

impl SessionWatcher {
    pub async fn new() -> zbus::Result<Self> {
        let conn = zbus::Connection::session().await?;
        Ok(Self { conn })
    }

    pub fn with_connection(conn: zbus::Connection) -> Self {
        Self { conn }
    }

    async fn get_player(&self, bus_name: OwnedBusName) -> anyhow::Result<Option<Player>> {
        if !bus_name.starts_with("org.mpris.MediaPlayer2.") { return Ok(None); }
        let conn = self.conn.clone();
        let player = Player::new(conn, bus_name);
        Ok(Some(player))
    }

    pub async fn iter_player(&self, with_initial: bool) -> impl Stream<Item=anyhow::Result<Player>> {
        async_stream::try_stream! {
            let bus = DBusProxy::new(&self.conn).await?;

            let mut changes = bus
                .receive_name_owner_changed()
                .await?;

            if with_initial {
                let names = bus.list_names().await?;
                for bus_name in names.into_iter() {
                    if let Some(player) = self.get_player(bus_name).await? {
                        yield player;
                    }
                }
            }

            use futures_util::StreamExt;

            while let Some(signal) = changes.next().await {
                let args = signal.args()?;
                // Only consider appearances (new owner present)
                if args.new_owner().is_none() { continue; }
                let bus_name = args.name().to_owned().into();

                if let Some(player) = self.get_player(bus_name).await? {
                    yield player;
                }
            }
        }
    }
}