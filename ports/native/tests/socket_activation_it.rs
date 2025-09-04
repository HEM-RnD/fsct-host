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
use std::os::fd::{AsRawFd as _, IntoRawFd as _};
use std::os::unix::process::CommandExt;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::io;
use std::os::unix::net::UnixListener;
use std::os::fd::AsRawFd;
use std::fs;
use nix::poll::PollTimeout;

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

    // Start a background client thread BEFORE spawning the service to emulate systemd-triggered activation.
    // The thread will repeatedly try to connect and, once connected, perform a msgpack-rpc call: get_protocol_version.
    use std::thread;


    // Create stdout/stderr pipes first and start reading threads before spawning child
    use nix::unistd::pipe2;
    use nix::fcntl::OFlag;
    use std::os::fd::FromRawFd;
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    // Create pipes with CLOEXEC initially; we'll clear CLOEXEC on the write ends passed to the child
    let (out_r, out_w) = pipe2(OFlag::O_CLOEXEC).expect("pipe2 stdout failed");
    let (err_r, err_w) = pipe2(OFlag::O_CLOEXEC).expect("pipe2 stderr failed");

    // Start reader threads on the read ends immediately
    use std::os::fd::OwnedFd;
    let out_reader_fd: OwnedFd = out_r;
    let err_reader_fd: OwnedFd = err_r;
    let out_reader = unsafe { File::from_raw_fd(out_reader_fd.into_raw_fd()) };
    let err_reader = unsafe { File::from_raw_fd(err_reader_fd.into_raw_fd()) };

    let out_handle = std::thread::spawn(move || {
        let mut reader = BufReader::new(out_reader);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 { break; }
            print!("[CHILD][stdout] {}", line);
            line.clear();
        }
    });
    let err_handle = std::thread::spawn(move || {
        let mut reader = BufReader::new(err_reader);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 { break; }
            print!("[CHILD][stderr] {}", line);
            line.clear();
        }
    });

    // Prepare write ends to hand to the child; transfer ownership via into_raw_fd when building Stdio
    let out_w_fd: OwnedFd = out_w;
    let err_w_fd: OwnedFd = err_w;


    let endpoint_str = sock_path.to_string_lossy().to_string();
    let client_handle = thread::spawn(move || {
        use parity_tokio_ipc::Endpoint;
        use tokio_util::compat::TokioAsyncReadCompatExt;
        // Create a small Tokio runtime inside the thread
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            // we want to be sure that main thread has already started waiting for the socket before we connect to it, so we wait a bit
            tokio::time::sleep(Duration::from_millis(300)).await;
            // we try only once to connect to the socket, because we want to be sure that service may run after a connection attempt and handle an incoming connection which triggers socket activation
            println!("[CLIENT] Connecting to {}", endpoint_str);
            let client = match Endpoint::connect(endpoint_str.clone()).await {
                Ok(stream) => msgpack_rpc::Client::new(stream.compat()),
                Err(_) => {
                    panic!("client failed to connect in time to {}", endpoint_str);
                }
            };
            // Call get_protocol_version and assert shape
            println!("[CLIENT] Connected, sending get_protocol_version request");
            let ver = client.request("get_protocol_version", &[]).await
                .expect("get_protocol_version request failed");
            // Expect a map { major: <int>, minor: <int> }
            let m = ver.as_map().expect("protocol version is not a map");
            let mut major = None;
            let mut minor = None;
            for (k, v) in m.iter() {
                if let msgpack_rpc::Value::String(s) = k {
                    if let Some(ks) = s.as_str() {
                        match ks {
                            "major" => { major = v.as_u64(); }
                            "minor" => { minor = v.as_u64(); }
                            _ => {}
                        }
                    }
                }
            }
            let (maj, min) = (major.unwrap_or(0), minor.unwrap_or(0));
            println!("[CLIENT] Read protocol version: {}.{}", maj, min);
            assert!(maj > 0 || min >= 0, "invalid protocol version map: {:?}", ver);
        });
    });

    // Wait until we detect a connection attempt on the listening socket (POLLIN),
    // then spawn the fsct_driver_service which should adopt fd=3 and accept the pending client.
    {
        println!("[SOCKET] Waiting for first incoming connection on {}", sock_path.display());
        use nix::poll::{poll, PollFd, PollFlags};
        use std::os::fd::{RawFd, BorrowedFd};
        let raw: RawFd = listener.as_raw_fd();
        let start = Instant::now();
        let timeout_total = Duration::from_secs(10);
        loop {
            // nix 0.29 requires BorrowedFd for PollFd::new
            let bfd = unsafe { BorrowedFd::borrow_raw(raw) };
            let mut pfd = [PollFd::new(bfd, PollFlags::POLLIN)];
            let poll_timeout: PollTimeout = Duration::from_millis(200).try_into().unwrap();
            let n = poll(&mut pfd, poll_timeout).expect("poll failed"); // 200 ms step
            if n > 0 {
                // Either a connection is pending or an error/hup occurred; proceed to spawn the service.
                break;
            }
            if start.elapsed() > timeout_total {
                panic!("timeout waiting for first incoming connection on {}", sock_path.display());
            }
        }
    }

    println!("[SOCKET] Starting fsct_driver_service");

    let fd = listener.as_raw_fd();
    let mut cmd = Command::new(&fsct_bin);
    cmd.arg("--driver");


    // Convert write ends into Stdio objects for child's stdout/stderr (transfer ownership)
    use std::process::Stdio;
    let child_stdout_stdio = unsafe { Stdio::from(File::from_raw_fd(out_w_fd.into_raw_fd())) };
    let child_stderr_stdio = unsafe { Stdio::from(File::from_raw_fd(err_w_fd.into_raw_fd())) };
    cmd.stdout(child_stdout_stdio);
    cmd.stderr(child_stderr_stdio);

    unsafe {
        cmd.pre_exec(move || {
            if dup2(fd, 3) < 0 { return Err(io::Error::last_os_error()); }
            clear_cloexec(3)?;
            let _ = clear_cloexec(fd);
            Ok(())
        });
    }
    cmd.env("LISTEN_FDS", "1");
    cmd.env("FSCT_LOG", "info");

    let mut child = cmd.spawn().expect("failed to spawn fsct service");

    // Wait for the client to finish RPC check
    client_handle.join().expect("client thread panicked");
    println!("[TEST] Client finished");

    // Cleanup process and socket
    let _ = terminate_child(&mut child);
    println!("[TEST] Service terminated");

    // Drop fd owners to close stdio handles
    drop(child);
    drop(cmd);

    // Join forwarders to flush remaining logs
    let _ = out_handle.join();
    let _ = err_handle.join();
    println!("[TEST] All done.");

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
