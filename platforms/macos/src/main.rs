
#[cfg(target_os = "macos")]
pub mod service;

#[cfg(target_os = "macos")]
pub mod player;
#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    service::fsct_main()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("This binary is only available when targeting macOS");
}
