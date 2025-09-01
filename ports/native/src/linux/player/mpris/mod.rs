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

    pub async fn iter_player(&self, with_initial: bool) -> impl Stream<Item=anyhow::Result<Player>> {
        let conn = self.conn.clone();
        async_stream::try_stream! {
            let bus = DBusProxy::new(&conn).await?;
            let rule = MatchRule::builder()
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .build();
            bus.add_match_rule(rule.clone()).await?;

            if with_initial {
                let names = bus.list_names().await?;
                for bus_name in names.into_iter().filter(|n| n.starts_with("org.mpris.MediaPlayer2.")) {
                    let name = bus_name.to_string();
                    let identity = match MediaPlayer2Proxy::builder(&conn).destination(name.as_str())?.build().await {
                        Ok(mp2) => mp2.identity().await.unwrap_or_else(|_| name.clone()),
                        Err(_) => name.clone(),
                    };
                    yield Player { conn: conn.clone(), name, identity };
                }
            }

            let mut msg_stream = zbus::MessageStream::from(&conn);
            use futures_util::StreamExt;
            while let Some(msg) = msg_stream.next().await {
                let msg = msg?;
                let iface_ok = msg.header().interface().map(|i| i.as_str() == "org.freedesktop.DBus").unwrap_or(false);
                let member_ok = msg.header().member().map(|m| m.as_str() == "NameOwnerChanged").unwrap_or(false);
                if !iface_ok || !member_ok { continue; }

                let body = msg.body();
                let (name, _old_owner, new_owner): (String, String, String) = body.deserialize()?;
                if !name.starts_with("org.mpris.MediaPlayer2.") { continue; }
                if new_owner.is_empty() { continue; }

                let identity = match MediaPlayer2Proxy::builder(&conn).destination(name.as_str())?.build().await {
                    Ok(mp2) => mp2.identity().await.unwrap_or_else(|_| name.clone()),
                    Err(_) => name.clone(),
                };
                yield Player { conn: conn.clone(), name, identity };
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


