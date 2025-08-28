#[cfg(target_os = "linux")]
pub mod service;

#[cfg(target_os = "linux")]
pub mod player;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    service::fsct_main()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("This binary is only available when targeting Linux");
}
