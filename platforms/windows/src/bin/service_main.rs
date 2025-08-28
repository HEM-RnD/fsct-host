#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    fsct_platform_windows::fsct_main()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("This binary is only available when targeting Windows");
}
