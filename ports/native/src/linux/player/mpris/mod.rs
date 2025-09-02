// Internal MPRIS adapter: hide zbus details and expose typed events for the watcher.
// Copyright 2025 HEM Sp. z o.o.

use futures_util::Stream;
use zbus::export::ordered_stream::OrderedStreamExt;
mod watcher;
mod media_player2;
mod media_player2_player;
mod player;

pub use watcher::SessionWatcher;
pub use player::Player;
pub use media_player2_player::PlaybackStatus;
pub use media_player2_player::PlayerProxy;
pub use media_player2::MediaPlayer2Proxy;
pub use media_player2_player::LoopStatus;

