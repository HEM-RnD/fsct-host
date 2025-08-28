// Linux player module placeholder

#[cfg(target_os = "linux")]
pub async fn run_os_watcher<T>(_driver: std::sync::Arc<T>) -> anyhow::Result<()> {
    println!("fsct-platform-linux: run_os_watcher placeholder");
    Ok(())
}
