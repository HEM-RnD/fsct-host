// Internal MPRIS adapter: hide zbus details and expose typed events for the watcher.
// Copyright 2025 HEM Sp. z o.o.

mod player;

use futures_util::Stream;


mod media_player2;
mod watcher;

use media_player2::*;
use player::*;

pub use watcher::SessionWatcher;

pub struct Player {
    conn: zbus::Connection,
    pub name: String,
    pub identity: String,
    // state: PlayerState,
    // owner: Option<String>,
}


