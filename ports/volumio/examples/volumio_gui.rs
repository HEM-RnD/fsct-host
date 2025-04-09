use env_logger;
use fsct_gui::run_gui;
use fsct_volumio_platform::create_rest_api_volumio_player;

#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();

    let volumio_url = "http://streamplay.local/";
    let player = create_rest_api_volumio_player(volumio_url).await.map_err(|e| e.to_string())?;
    run_gui(player).await?;

    Ok(())
}
