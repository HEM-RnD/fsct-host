/// Returns the default path of the Unix Domain Socket used by FSCT IPC on Linux.
#[cfg(target_os = "linux")]
pub fn default_endpoint_path() -> &'static str {
    "/run/fsct/fsct.sock"
}

/// Returns the default path of the Named Pipe used by FSCT IPC on Windows.
#[cfg(target_os = "windows")]
pub fn default_endpoint_path() -> &'static str {
    "\\\\.\\pipe\\fsct_driver"
}

/// Returns the default path of the Unix Domain Socket used by FSCT IPC on macOS.
#[cfg(target_os = "macos")]
pub fn default_endpoint_path() -> &'static str {
    "/var/run/fsct/fsct.sock"
}
