use env_logger;
use fsct_native_port::initialize_native_platform_player;
use fsct_gui::run_gui;


#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();

    let player = initialize_native_platform_player().await?;
    run_gui(player).await?;

    Ok(())
}
