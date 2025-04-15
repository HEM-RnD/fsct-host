use fsct_core::player::Player;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

#[allow(unreachable_code)]

pub async fn initialize_native_platform_player() -> Result<Player, String> {
    #[cfg(target_os = "windows")]
    {
        let windows_player = windows::WindowsPlatformGlobalSessionManager::new()
            .await
            .map_err(|e| e.to_string())?;
        return Ok(Player::new(windows_player));
    }
    #[cfg(target_os = "macos")]
    {
        return Ok(Player::new(
            macos::MacOSPlaybackManager::new().map_err(|e| e.to_string())?,
        ));
    }
    {
        panic!("Unsupported platform");
    }
}
