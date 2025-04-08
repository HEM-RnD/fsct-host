use dac_player_integration::platform;
use dac_player_integration::player::{Player, PlayerInterface};

async fn print_current_track(player: &Player) -> Result<(), String> {
    let track = player
        .get_current_track()
        .await
        .map_err(|e| e.to_string())?;

    println!("{:#?}", track);
    Ok(())
}

async fn print_current_track_pos(
    player: &Player,
) -> Result<(), String> {
    let timeline = player
        .get_timeline_info()
        .await
        .map_err(|e| e.to_string())?;
    println!("{:#?}", timeline);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let platform = platform::get_platform();
    let player = platform.initialize().await?;
    for _ in 0..100 {
        print_current_track(&player).await?;
        print_current_track_pos(&player).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    Ok(())
}
