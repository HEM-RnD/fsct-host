
#[cfg(target_os = "windows")]
pub mod service;
#[cfg(target_os = "windows")]
pub mod player;


#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    service::fsct_main()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("This binary is only available when targeting Windows");
}
