use env_logger;
use fsct_core::run_service;
use log::info;
use fsct_volumio_port::create_rest_api_volumio_player;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    env_logger::init();

    let url = std::env::var("FSCT_VOLUMIO_URL").unwrap_or("http://localhost/".to_string());
    info!("Using volumio url: {}", url);

    let platform_global_player = create_rest_api_volumio_player(url.as_str()).await.map_err
    (|e| e.to_string())?;
    run_service(platform_global_player).await
}