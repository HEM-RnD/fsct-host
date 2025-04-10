use fsct_core::run_service;
use fsct_native_port::initialize_native_platform_player;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    env_logger::init();
    let platform_global_player = initialize_native_platform_player().await?;
    run_service(platform_global_player).await?;

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C signal");
    println!("Exiting...");
    Ok(())
}