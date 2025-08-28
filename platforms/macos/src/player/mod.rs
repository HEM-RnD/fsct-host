// macOS player module placeholder

#[cfg(target_os = "macos")]
pub async fn run_os_watcher<T>(_driver: std::sync::Arc<T>) -> anyhow::Result<()> {
    println!("fsct-platform-macos: run_os_watcher placeholder");
    Ok(())
}
