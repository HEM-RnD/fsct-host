// Internal MPRIS adapter: hide zbus details and expose typed events for the watcher.
// Copyright 2025 HEM Sp. z o.o.

use std::collections::HashMap;
use zbus::{Connection, MessageStream, MatchRule};
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy::Proxy;
use zbus::zvariant::OwnedValue;
use fsct_core::player_state::PlayerState;
use futures_util::{StreamExt, stream};

#[derive(Debug)]
pub enum MprisEvent {
    PlayerAppeared { name: String, identity: String, state: PlayerState, owner: Option<String> },
    PlayerDisappeared { name: String },
    PropertiesChanged { name: String, changed: HashMap<String, OwnedValue>, sender: Option<String> },
    Seeked { name: String, pos_us: i64, sender: Option<String> },
}

pub async fn install_match_rules(conn: &Connection) -> zbus::Result<()> {
    let bus = DBusProxy::new(conn).await?;
    // NameOwnerChanged
    let rule = MatchRule::builder()
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .build();
    bus.add_match_rule(rule).await?;
    // PropertiesChanged on MPRIS path
    let rule = MatchRule::builder()
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path("/org/mpris/MediaPlayer2")?
        .build();
    bus.add_match_rule(rule).await?;
    // Seeked on MPRIS player
    let rule = MatchRule::builder()
        .interface("org.mpris.MediaPlayer2.Player")?
        .member("Seeked")?
        .path("/org/mpris/MediaPlayer2")?
        .build();
    bus.add_match_rule(rule).await?;
    Ok(())
}

pub async fn initial_players(conn: &Connection) -> zbus::Result<Vec<MprisEvent>> {
    let mut out = Vec::new();
    let dbus = DBusProxy::new(conn).await?;
    let names = dbus.list_names().await?;
    for owned in names {
        let name = owned.to_string();
        if !name.starts_with("org.mpris.MediaPlayer2.") { continue; }
        if let Ok((identity, state)) = get_initial(conn, &name).await {
            // owner may be absent
            let owner = if let Ok(bn) = BusName::try_from(name.as_str()) {
                dbus.get_name_owner(bn).await.ok().map(|o| o.to_string())
            } else { None };
            out.push(MprisEvent::PlayerAppeared { name, identity, state, owner });
        }
    }
    Ok(out)
}

pub struct MprisStream {
    conn: Connection,
}

impl MprisStream {
    pub fn new(conn: Connection) -> Self { Self { conn } }

    pub fn into_stream(self) -> impl stream::Stream<Item = zbus::Result<MprisEvent>> {
        let mut msg_stream = MessageStream::from(&self.conn);
        let conn = self.conn.clone();
        stream::unfold((msg_stream, conn), |(mut msg_stream, conn)| async move {
            match msg_stream.next().await {
                Some(Ok(msg)) => {
                    let hdr = msg.header();
                    // Produce at most one event per message for simplicity
                    if hdr.member().map(|m| m.as_str()) == Some("NameOwnerChanged") {
                        if let Ok((name, old_owner, new_owner)) = msg.body().deserialize::<(String, String, String)>() {
                            if name.starts_with("org.mpris.MediaPlayer2.") {
                                if old_owner.is_empty() && !new_owner.is_empty() {
                                    if let Ok((identity, state)) = get_initial(&conn, &name).await {
                                        let evt = MprisEvent::PlayerAppeared { name, identity, state, owner: Some(new_owner) };
                                        return Some((Ok(evt), (msg_stream, conn)));
                                    }
                                } else if !old_owner.is_empty() && new_owner.is_empty() {
                                    let evt = MprisEvent::PlayerDisappeared { name };
                                    return Some((Ok(evt), (msg_stream, conn)));
                                }
                            }
                        }
                        Some((Ok::<MprisEvent, zbus::Error>(MprisEvent::PlayerDisappeared { name: String::from("") }), (msg_stream, conn)))
                    } else if hdr.member().map(|m| m.as_str()) == Some("PropertiesChanged") {
                        if let Ok((iface, changed, _)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() {
                            if iface == "org.mpris.MediaPlayer2.Player" {
                                let sender = hdr.sender().map(|s| s.to_string());
                                let evt = MprisEvent::PropertiesChanged { name: String::new(), changed, sender };
                                return Some((Ok(evt), (msg_stream, conn)));
                            }
                        }
                        Some((Ok::<MprisEvent, zbus::Error>(MprisEvent::PlayerDisappeared { name: String::from("") }), (msg_stream, conn)))
                    } else if hdr.member().map(|m| m.as_str()) == Some("Seeked") {
                        if let Ok((pos_us,)) = msg.body().deserialize::<(i64,)>() {
                            let sender = hdr.sender().map(|s| s.to_string());
                            let evt = MprisEvent::Seeked { name: String::new(), pos_us, sender };
                            return Some((Ok(evt), (msg_stream, conn)));
                        }
                        Some((Ok::<MprisEvent, zbus::Error>(MprisEvent::PlayerDisappeared { name: String::from("") }), (msg_stream, conn)))
                    } else {
                        Some((Ok::<MprisEvent, zbus::Error>(MprisEvent::PlayerDisappeared { name: String::from("") }), (msg_stream, conn)))
                    }
                }
                Some(Err(e)) => Some((Err(e), (msg_stream, conn))),
                None => None,
            }
        })
    }
}

pub async fn get_initial(conn: &Connection, bus: &str) -> zbus::Result<(String, PlayerState)> {
    let path = "/org/mpris/MediaPlayer2";
    let mp2 = Proxy::new(conn, bus, path, "org.mpris.MediaPlayer2").await?;
    let player = Proxy::new(conn, bus, path, "org.mpris.MediaPlayer2.Player").await?;

    let identity: String = mp2.get_property("Identity").await?;
    let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata").await?;
    let playback_status: Option<String> = player.get_property("PlaybackStatus").await.ok();
    let position_us: Option<i64> = player.get_property("Position").await.ok();
    let playback_rate: Option<f64> = player.get_property("Rate").await.ok();
    let state = crate::linux::player::build_state_from_map(&metadata, playback_status.as_deref(), position_us, playback_rate);
    Ok((identity, state))
}

pub async fn get_rate(conn: &Connection, name: &str) -> zbus::Result<f64> {
    let p = Proxy::new(conn, name, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await?;
    let rate: f64 = p.get_property("Rate").await?;
    Ok(rate)
}

pub async fn get_position(conn: &Connection, name: &str) -> zbus::Result<i64> {
    let p = Proxy::new(conn, name, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await?;
    let pos: i64 = p.get_property("Position").await?;
    Ok(pos)
}

pub async fn get_metadata(conn: &Connection, name: &str) -> zbus::Result<HashMap<String, OwnedValue>> {
    let p = Proxy::new(conn, name, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player").await?;
    let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").await?;
    Ok(meta)
}
