#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    fsct_platform_linux::fsct_main()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("This binary is only available when targeting Linux");
}
