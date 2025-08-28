// Windows player module placeholder

#[cfg(target_os = "windows")]
pub async fn run_os_watcher<T>(_driver: std::sync::Arc<T>) -> anyhow::Result<()> {
    println!("fsct-platform-windows: run_os_watcher placeholder");
    Ok(())
}
