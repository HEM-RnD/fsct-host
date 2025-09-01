use futures_util::Stream;
use zbus::fdo::DBusProxy;
use crate::linux::player::mpris::{media_player2::MediaPlayer2Proxy, Player};

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

    async fn get_player(&self, name: &str) -> anyhow::Result<Option<Player>> {
        if !name.starts_with("org.mpris.MediaPlayer2.") { return Ok(None); }
        let name = name.to_string();
        let conn = self.conn.clone();
        let player = Player { conn, name };
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