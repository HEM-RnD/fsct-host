use env_logger::Env;
use fsct_core::run_service;
use fsct_native_port::initialize_native_platform_player;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let env = Env::default()
        .filter_or("FSCT_LOG", "info")
        .write_style("FSCT_LOG_STYLE");
    env_logger::init_from_env(env);

    let platform_global_player = initialize_native_platform_player().await?;
    run_service(platform_global_player).await?;

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C signal");
    println!("Exiting...");
    Ok(())
}