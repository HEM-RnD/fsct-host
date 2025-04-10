use env_logger;
use fsct_gui::run_gui;
use fsct_volumio_platform::create_rest_api_volumio_player;

#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();

    let volumio_url = std::env::var("VOLUMIO_URL").unwrap_or("http://volumio.local/".to_string());

    let player = create_rest_api_volumio_player(volumio_url.as_str()).await.map_err(|e| e.to_string())?;
    run_gui(player).await?;

    Ok(())
}
