use zbus::names::OwnedBusName;
use zbus::fdo::DBusProxy;
use anyhow::bail;
use zbus::export::ordered_stream::OrderedStreamExt;
use crate::linux::player::mpris::{media_player2, media_player2_player};

pub struct Player {
    conn: zbus::Connection,
    bus_name: OwnedBusName,
}

impl Player {
    pub fn new(conn: zbus::Connection, bus_name: OwnedBusName) -> Self {
        Self { conn, bus_name }
    }
    
    pub async fn as_media_player2_interface(&self) -> anyhow::Result<media_player2::MediaPlayer2Proxy<'_>>
    {
        Ok(media_player2::MediaPlayer2Proxy::new(&self.conn, self.bus_name.clone()).await?)
    }

    pub async fn as_player_interface(&self) -> anyhow::Result<media_player2_player::PlayerProxy<'_>>
    {
        Ok(media_player2_player::PlayerProxy::new(&self.conn, self.bus_name.clone()).await?)
    }

    pub async fn wait_for_disconnect(&self) -> anyhow::Result<()>
    {
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
}