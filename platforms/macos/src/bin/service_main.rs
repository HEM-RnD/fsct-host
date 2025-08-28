#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    fsct_platform_macos::fsct_main()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("This binary is only available when targeting macOS");
}
