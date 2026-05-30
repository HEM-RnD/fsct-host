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

use crate::ports::linux::player::mpris::{media_player2, media_player2_player};
use anyhow::bail;
use zbus::export::ordered_stream::OrderedStreamExt;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::OwnedBusName;

pub struct Player {
    conn: zbus::Connection,
    bus_name: OwnedBusName,
}

impl Player {
    pub fn new(conn: zbus::Connection, bus_name: OwnedBusName) -> Self {
        Self { conn, bus_name }
    }

    pub async fn as_media_player2_interface(&self) -> anyhow::Result<media_player2::MediaPlayer2Proxy<'_>> {
        Ok(media_player2::MediaPlayer2Proxy::new(&self.conn, self.bus_name.clone()).await?)
    }

    pub async fn as_player_interface(&self) -> anyhow::Result<media_player2_player::PlayerProxy<'_>> {
        Ok(media_player2_player::PlayerProxy::new(&self.conn, self.bus_name.clone()).await?)
    }

    pub async fn wait_for_disconnect(&self) -> anyhow::Result<()> {
        let dbus = DBusProxy::new(&self.conn).await?;
        let mut name_owner_changed = dbus.receive_name_owner_changed().await?;
        while let Some(event) = name_owner_changed.next().await {
            let args = event.args()?;
            if args.name() != self.bus_name.as_str() {
                continue;
            }
            if args.new_owner().is_none() {
                return Ok(());
            }
        }
        bail!("Player disconnected unexpectedly");
    }

    pub fn name(&self) -> String {
        self.bus_name.to_string()
    }

    pub async fn position_uncached(&self) -> anyhow::Result<i64> {
        // Query Position via org.freedesktop.DBus.Properties to bypass proxy caching
        let props = PropertiesProxy::builder(&self.conn)
            .destination(self.bus_name.clone())?
            .path("/org/mpris/MediaPlayer2")?
            .build()
            .await?;
        let iface = zbus::names::InterfaceName::try_from("org.mpris.MediaPlayer2.Player")?;
        let val: zbus::zvariant::OwnedValue = props.get(iface, "Position").await?;
        // Try to extract i64, fallback to i32
        if let Ok(us) = <i64 as std::convert::TryFrom<zbus::zvariant::OwnedValue>>::try_from(val.clone()) {
            Ok(us)
        } else if let Ok(us32) = <i32 as std::convert::TryFrom<zbus::zvariant::OwnedValue>>::try_from(val) {
            Ok(us32 as i64)
        } else {
            bail!("Unexpected type for Position property")
        }
    }
}
