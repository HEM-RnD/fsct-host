// Internal MPRIS adapter: hide zbus details and expose typed events for the watcher.
// Copyright 2025 HEM Sp. z o.o.

mod player;

use futures_util::Stream;
use zbus::fdo::DBusProxy;
use zbus::MatchRule;
use fsct_core::{PlayerEvent, PlayerState};
pub use player::*;

mod media_player2;
pub use media_player2::*;

pub struct PlayerWatcher {
    conn: zbus::Connection,
}

impl PlayerWatcher {
    pub async fn new() -> zbus::Result<Self> {
        let conn = zbus::Connection::session().await?;
        Ok(Self { conn })
    }

    pub fn with_connection(conn: zbus::Connection) -> Self {
        Self { conn }
    }

    async fn get_player(&self, name: &str) -> anyhow::Result<Option<Player>> {
        if !name.starts_with("org.mpris.MediaPlayer2.") { return Ok(None); }
        let name = name.to_string();
        let conn = self.conn.clone();
        let identity = match MediaPlayer2Proxy::builder(&conn).destination(name.as_str())?.build().await {
            Ok(mp2) => mp2.identity().await.unwrap_or_else(|_| name.clone()),
            Err(_) => name.clone(),
        };
        let player = Player { conn, name, identity };
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
                    if let Some(player) = self.get_player(bus_name.as_str()).await? {
                        yield player;
                    }
                }
            }

            use futures_util::StreamExt;

            while let Some(signal) = changes.next().await {
                let args = signal.args()?;
                // Only consider appearances (new owner present)
                if args.new_owner().is_none() { continue; }

                if let Some(player) = self.get_player(args.name().as_str()).await? {
                    yield player;
                }
            }
        }
    }
}

pub struct Player {
    conn: zbus::Connection,
    pub name: String,
    pub identity: String,
    // state: PlayerState,
    // owner: Option<String>,
}


