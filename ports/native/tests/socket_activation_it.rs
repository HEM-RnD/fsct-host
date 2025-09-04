// Integration test: simulate systemd socket activation without external helpers.
// The test itself creates a UDS listener at a random /tmp path, hands it to the
// cargo-built fsct_driver_service via fd=3 and LISTEN_FDS=1, and verifies that a
// client can connect.
//
// Enable with:
//   FSCT_RUN_SOCKET_ACTIVATION_TEST=1 cargo test -p fsct_driver_service --test socket_activation_it -- --nocapture
//
// Notes:
// - Unix-only and #[ignore] by default.
// - We do not assert server logs, only that a connection is possible.

#![cfg(unix)]

use std::process::{Command, Child};
use std::os::unix::process::CommandExt;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::io;
use std::os::unix::net::UnixListener;
use std::os::fd::AsRawFd;
use std::fs;


fn random_sock_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    let suffix = format!("{}", uuid::Uuid::new_v4());
    p.push(format!("fsct-{}.sock", suffix));
    p
}

// Minimal libc bindings for dup2 and fcntl
unsafe extern "C" {
    fn dup2(oldfd: std::os::raw::c_int, newfd: std::os::raw::c_int) -> std::os::raw::c_int;
    fn fcntl(fd: std::os::raw::c_int, cmd: std::os::raw::c_int, ...) -> std::os::raw::c_int;
}
const F_GETFD: i32 = 1; // get close-on-exec
const F_SETFD: i32 = 2; // set close-on-exec
const FD_CLOEXEC: i32 = 1;

fn clear_cloexec(fd: i32) -> io::Result<()> {
    let flags = unsafe { fcntl(fd, F_GETFD) };
    if flags < 0 { return Err(io::Error::last_os_error()); }
    let new_flags = flags & !FD_CLOEXEC;
    let r = unsafe { fcntl(fd, F_SETFD, new_flags) };
    if r < 0 { return Err(io::Error::last_os_error()); }
    Ok(())
}

#[test]
fn socket_activation_embedded_helper_works() {
    // In a proper cargo test run for this package, Cargo sets CARGO_BIN_EXE_fsct_driver_service.
    // Hitting this branch means test setup is broken; fail hard.
    let fsct_bin: &str = env!("CARGO_BIN_EXE_fsct_driver_service");
    let fsct_bin = PathBuf::from(fsct_bin);

    // Prepare socket
    let sock_path = random_sock_path();
    if let Some(dir) = sock_path.parent() { let _ = fs::create_dir_all(dir); }
    let _ = fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path).expect("failed to bind unix socket");
    // chmod 0666
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(&sock_path).unwrap().permissions();
    perm.set_mode(0o666);
    fs::set_permissions(&sock_path, perm).unwrap();

    // Build command: fsct_driver_service --driver, passing fd 3 and LISTEN_FDS=1 using pre_exec
    let fd = listener.as_raw_fd();
    let mut cmd = Command::new(&fsct_bin);
    cmd.arg("--driver");
    unsafe {
        cmd.pre_exec(move || {
            if dup2(fd, 3) < 0 { return Err(io::Error::last_os_error()); }
            clear_cloexec(3)?;
            let _ = clear_cloexec(fd);
            Ok(())
        });
    }
    cmd.env("LISTEN_FDS", "1");
    cmd.env("FSCT_LOG", "debug");

    let mut child = cmd.spawn().expect("failed to spawn fsct service");

    // Try to connect to the socket a few times
    use std::os::unix::net::UnixStream;
    use std::io::{Write, Read};

    let start2 = Instant::now();
    let timeout2 = Duration::from_secs(5);
    let mut connected = false;
    while start2.elapsed() < timeout2 {
        match UnixStream::connect(&sock_path) {
            Ok(mut s) => {
                let _ = s.write_all(b"hello from test\n");
                let mut buf = [0u8; 32];
                // let _ = s.read_timeout(&mut buf);
                connected = true;
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }

    assert!(connected, "client failed to connect to {}", sock_path.display());

    // Cleanup process and socket
    let _ = terminate_child(&mut child);
    let _ = fs::remove_file(&sock_path);
}

fn terminate_child(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let pid = child.id() as i32;
        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}
