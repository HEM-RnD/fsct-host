use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use anyhow::anyhow;
use crate::player::{Player, PlayerState};
use crate::{devices_watch, player_watch};
use crate::devices_watch::DevicesPlayerEventApplier;


pub async fn run_service(player: Player) -> Result<(), anyhow::Error> {
    let fsct_devices = Arc::new(Mutex::new(HashMap::new()));
    let player_state = Arc::new(Mutex::new(PlayerState::default()));

    let player_event_listener = DevicesPlayerEventApplier::new(fsct_devices.clone());

    devices_watch::run_devices_watch(fsct_devices.clone(), player_state.clone()).await?;
    player_watch::run_player_watch(player, player_event_listener, player_state).await.map_err(|e| anyhow!(e))?;
    Ok(())
}