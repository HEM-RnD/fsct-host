#![cfg(target_os = "linux")]
// Integration test: simulate systemd socket activation without external helpers.
// The test itself creates a UDS listener at a random /tmp path, hands it to the
// cargo-built fsct_driver via fd=3 and LISTEN_FDS=1, and verifies that a
// client can connect.
//
// Enable with:
//   FSCT_RUN_SOCKET_ACTIVATION_TEST=1 cargo test -p fsct_driver --test socket_activation_it -- --nocapture
//
// Notes:
// - Unix-only and #[ignore] by default.
// - We do not assert server logs, only that a connection is possible.

use std::os::fd::IntoRawFd as _;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::io;
use std::os::unix::net::UnixListener;
use std::os::fd::AsRawFd;
use std::fs;
use anyhow::{anyhow, bail, Context};
use nix::libc::setenv;
use nix::poll::PollTimeout;
use fsct::{ProtocolVersion, FSCT_PROTOCOL_VERSION};
use std::os::unix::ffi::OsStrExt;
use nix::libc as libc;
use tokio::net::UnixStream;

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
    // fn setenv(name: *const i8, value: *const i8, overwrite: std::os::raw::c_int) -> std::os::raw::c_int;
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

struct ChildHandle {
    pid: i32,
}

fn fork_exec_service(fsct_bin: &PathBuf, listener_fd: i32, out_wr_fd: i32, err_wr_fd: i32) -> io::Result<ChildHandle> {
    use std::ffi::CString;
    unsafe {
        // Prepare argv
        let prog = CString::new(fsct_bin.as_os_str().as_bytes()).unwrap();
        let arg0 = prog.clone();
        let arg1 = CString::new("--driver").unwrap();
        let argv: [*const i8; 3] = [arg0.as_ptr(), arg1.as_ptr(), std::ptr::null()];

        // Fork
        let pid = nix::unistd::fork().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        match pid {
            nix::unistd::ForkResult::Parent { child } => {
                // Parent returns handle
                return Ok(ChildHandle { pid: child.as_raw() });
            }
            nix::unistd::ForkResult::Child => {
                // Child process setup
                // dup listener to fd 3
                if dup2(listener_fd, 3) < 0 { libc::_exit(127); }
                // clear CLOEXEC on 3
                if clear_cloexec(3).is_err() { libc::_exit(127); }

                // Wire stdout/stderr
                if dup2(out_wr_fd, 1) < 0 { libc::_exit(127); }
                if dup2(err_wr_fd, 2) < 0 { libc::_exit(127); }
                // Clear CLOEXEC on 1,2 as well
                let _ = clear_cloexec(1);
                let _ = clear_cloexec(2);

                // Set env: LISTEN_FDS=1, LISTEN_PID=<child_pid>, FSCT_LOG=info
                let one = CString::new("1").unwrap();
                setenv(b"LISTEN_FDS\0".as_ptr() as *const i8, one.as_ptr(), 1);
                let pid = libc::getpid();
                let pid_str = CString::new(format!("{}", pid)).unwrap();
                setenv(b"LISTEN_PID\0".as_ptr() as *const i8, pid_str.as_ptr(), 1);
                let log = CString::new("info").unwrap();
                setenv(b"FSCT_LOG\0".as_ptr() as *const i8, log.as_ptr(), 1);

                // Exec
                libc::execvp(prog.as_ptr(), argv.as_ptr());
                // If exec failed
                libc::_exit(127);
            }
        }
    }
}

fn terminate_child(handle: &mut ChildHandle) -> std::io::Result<()> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::{Pid};
    let _ = kill(Pid::from_raw(handle.pid), Signal::SIGTERM);
    // wait
    let _ = nix::sys::wait::waitpid(Pid::from_raw(handle.pid), None);
    Ok(())
}

#[test]
fn socket_activation_correctly_passes_socket_fd_into_service_and_service_accepts_connection() {
    // In a proper cargo test run for this package, Cargo sets CARGO_BIN_EXE_fsct_driver.
    // Hitting this branch means test setup is broken; fail hard.
    let fsct_bin: &str = env!("CARGO_BIN_EXE_fsctd");
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
    let (out_rd_fd, out_wr_fd) = pipe2(OFlag::O_CLOEXEC).expect("pipe2 stdout failed");
    let (err_rd_fd, err_wr_fd) = pipe2(OFlag::O_CLOEXEC).expect("pipe2 stderr failed");

    // Start reader threads on the read ends immediately
    let out_reader = unsafe { File::from_raw_fd(out_rd_fd.into_raw_fd()) };
    let err_reader = unsafe { File::from_raw_fd(err_rd_fd.into_raw_fd()) };

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

    let endpoint_str = sock_path.to_string_lossy().to_string();
    let client_handle = thread::spawn(move || {
        use fsct_client::rpc::{RpcRequest, RpcResponse, MAX_LINE_BYTES};
        use tokio::time::timeout;
        use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
        use futures::{SinkExt, StreamExt};
        use serde_json::json;

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

            let connection = timeout(Duration::from_secs(2), UnixStream::connect(endpoint_str.as_str())).await
                .with_context(|| format!("client connection timed out after 5 seconds to {}", endpoint_str))?
                .with_context(|| format!("client failed to connect to {}: invalid endpoint", endpoint_str))?;

            let (rd, wr) = tokio::io::split(connection);
            let mut reader = FramedRead::new(rd, LinesCodec::new_with_max_length(MAX_LINE_BYTES));
            let mut writer = FramedWrite::new(wr, LinesCodec::new_with_max_length(MAX_LINE_BYTES));

            // Call get_protocol_version and assert shape
            println!("[CLIENT] Connected, sending get_protocol_version request");
            let req = RpcRequest {
                jsonrpc: "2.0".into(),
                id: json!(1u64),
                method: "get_protocol_version".into(),
                params: json!({}),
            };
            writer.send(serde_json::to_string(&req).unwrap()).await
                .with_context(|| "failed to send get_protocol_version request")?;

            let resp_line = timeout(Duration::from_secs(1), reader.next()).await
                .with_context(|| format!("get_protocol_version response timed out to {}", endpoint_str))?
                .ok_or_else(|| anyhow!("connection closed before get_protocol_version response"))?
                .with_context(|| "error reading get_protocol_version response")?;

            let resp: RpcResponse = serde_json::from_str(&resp_line)
                .with_context(|| format!("failed to parse protocol version response: {}", resp_line))?;

            if let Some(err) = resp.error {
                bail!("get_protocol_version returned error: {}", err.message);
            }

            let result = resp.result.with_context(|| "get_protocol_version returned no result")?;
            let major = result["major"].as_u64().with_context(|| "missing major in protocol version")? as u16;
            let minor = result["minor"].as_u64().with_context(|| "missing minor in protocol version")? as u16;

            let protocol_version = FSCT_PROTOCOL_VERSION;
            let read_version = ProtocolVersion { major, minor };
            println!("[CLIENT] Read protocol version: {}", read_version);
            if protocol_version != read_version {
                bail!("protocol version mismatch: expected {}, got {}", protocol_version, read_version);
            }
            Ok(())
        })
    });

    // Wait until we detect a connection attempt on the listening socket (POLLIN),
    // then spawn the fsct_driver which should adopt fd=3 and accept the pending client.
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

    println!("[SOCKET] Starting fsct_driver");

    let fd = listener.as_raw_fd();
    // Convert to raw fds and (optionally) clear CLOEXEC on them before passing
    let out_wr_raw = out_wr_fd.into_raw_fd();
    let err_wr_raw = err_wr_fd.into_raw_fd();
    let _ = clear_cloexec(out_wr_raw);
    let _ = clear_cloexec(err_wr_raw);
    let mut child = fork_exec_service(&fsct_bin, fd, out_wr_raw, err_wr_raw)
        .expect("failed to fork/exec fsct service");
    // Close our copies of the write ends in the parent so readers see EOF when child closes
    let _ = nix::unistd::close(out_wr_raw);
    let _ = nix::unistd::close(err_wr_raw);

    // Wait for the client to finish RPC check
    let res = client_handle.join().expect("client thread panicked");
    if res.is_err() {
        println!("[TEST] Client finished with error");
    } else {
        println!("[TEST] Client finished successfully");
    }

    // Cleanup process and socket
    let _ = terminate_child(&mut child);
    println!("[TEST] Service terminated");

    // Drop fd owners to close stdio handles

    // Join forwarders to flush remaining logs
    let _ = out_handle.join();
    let _ = err_handle.join();
    println!("[TEST] All done.");

    let _ = fs::remove_file(&sock_path);
    res.unwrap();
}

