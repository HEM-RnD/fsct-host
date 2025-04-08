use dac_player_integration::platform;
use dac_player_integration::run_service;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let platform = platform::get_platform();
    let platform_context = platform.initialize().await?;
    run_service(platform_context).await
}