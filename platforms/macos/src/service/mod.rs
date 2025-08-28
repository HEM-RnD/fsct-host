// macOS service module placeholder

use env_logger::Env;

#[cfg(target_os = "macos")]
pub async fn fsct_main() -> anyhow::Result<()> {
    let env = Env::default()
        .filter_or("FSCT_LOG", "info")
        .write_style("FSCT_LOG_STYLE");
    env_logger::init_from_env(env);
    println!("fsct-platform-macos: placeholder crate. Use ports/native during migration.");
    Ok(())
}
