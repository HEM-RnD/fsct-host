// Windows service module placeholder
// The actual implementation remains in ports/native for now.

#[cfg(target_os = "windows")]
pub fn fsct_main() -> anyhow::Result<()> {
    // For now, inform users that this crate is a placeholder until migration completes.
    println!("fsct-platform-windows: placeholder crate. Use ports/native on Windows until migration is complete.");
    Ok(())
}
