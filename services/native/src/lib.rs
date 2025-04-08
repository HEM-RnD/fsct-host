use std::sync::Arc;
use fsct_core::player::Player;

#[cfg(target_os = "windows")]
use fsct_windows_port;

#[cfg(target_os = "macos")]
use fsct_macos_port::macos;

#[allow(unreachable_code)]

pub async fn initialize_native_platform_player() -> Result<Player, String> {
    #[cfg(target_os = "windows")]
    {
        let windows_player = fsct_windows_port::WindowsPlatformGlobalSessionManager::new().await.map_err(|e| e.to_string())?;
        return Ok(Player::new(Arc::new(windows_player)));
    }
    #[cfg(target_os = "macos")]
    {
        return Ok(Player::new(Arc::new(macos::MacOSPlaybackManager::new().await?)));
    }
    {
        panic!("Unsupported platform");
    }
}