use fsct_core::run_service;


#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let platform_global_player = fsct_native_service::initialize_native_platform_player().await?;
    run_service(platform_global_player).await
}