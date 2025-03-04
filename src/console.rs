mod platform;

async fn print_current_track(platform_context: &platform::PlatformContext) -> Result<(), String> {
    let track = platform_context
        .info
        .get_current_track()
        .await
        .map_err(|e| e.to_string())?;

    println!("{:#?}", track);
    Ok(())
}

async fn print_current_track_pos(
    platform_context: &platform::PlatformContext,
) -> Result<(), String> {
    let timeline = platform_context
        .info
        .get_timeline_info()
        .await
        .map_err(|e| e.to_string())?;
    println!("{:#?}", timeline);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let platform = platform::get_platform();
    let platform_context = platform.initialize().await?;
    for _ in 0..100 {
        print_current_track(&platform_context).await?;
        print_current_track_pos(&platform_context).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    Ok(())
}
