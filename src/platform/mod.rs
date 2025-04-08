use async_trait::async_trait;
use crate::player::Player;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod linux;

#[async_trait]
pub trait PlatformBehavior {
    fn get_platform_name(&self) -> &'static str;
    async fn initialize(&self) -> Result<Player, String>;
    async fn cleanup(&self) -> Result<(), String>;
}

pub fn get_platform() -> Box<dyn PlatformBehavior> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOSPlatform::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform::new())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        panic!("Unsupported platform");
    }
} 