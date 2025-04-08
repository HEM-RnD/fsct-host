use std::sync::Arc;
use env_logger;
use reqwest::Url;
use fsct_core::player::Player;
use fsct_volumio_platform::VolumioPlayer;
use fsct_gui::run_gui;


#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();

    let volumio_url = Url::parse("http://streamplay.local/").map_err(|e| e.to_string())?;
    let player = Arc::new(VolumioPlayer::new(volumio_url).await.map_err(|e| e.to_string())?);
    run_gui(Player::new(player)).await?;

    Ok(())
}
