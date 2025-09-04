// Simple Unix Domain Socket client for manual testing.
//
// Usage:
//   cargo run -p fsct_driver_service --example uds_client -- /tmp/fsct-XXXX.sock "hello"
// or
//   FSCT_SOCK=/tmp/fsct-XXXX.sock cargo run -p fsct_driver_service --example uds_client -- "hello"
//
// It connects to the given socket and writes the optional message, then exits.
// The FSCT IPC server speaks a binary protocol; this client is only for triggering a connection
// and optionally sending a line to exercise the accept path.

use std::env;
use std::io::{self, Write, Read};
use std::os::unix::net::UnixStream;

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let (sock_path, msg) = match args.as_slice() {
        [path, rest @ ..] => (path.clone(), if rest.is_empty() { None } else { Some(rest.join(" ")) }),
        [] => {
            let path = env::var("FSCT_SOCK").map_err(|_| anyhow::anyhow!("provide socket path as arg or FSCT_SOCK env"))?;
            let msg = None;
            (path, msg)
        }
    };

    println!("[client] connecting to {}", sock_path);
    let mut stream = UnixStream::connect(&sock_path)?;
    stream.set_nonblocking(false)?;

    if let Some(m) = msg {
        let mut data = m.into_bytes();
        data.push(b'\n');
        stream.write_all(&data)?;
        stream.flush()?;
        // Try to read any immediate response (will likely just EOF or nothing for our server)
        let mut buf = [0u8; 256];
        if let Ok(n) = stream.read(&mut buf) {
            if n > 0 {
                println!("[client] read {} bytes: {}", n, String::from_utf8_lossy(&buf[..n]));
            }
        }
    } else {
        println!("[client] connected; no data sent");
    }

    println!("[client] done");
    Ok(())
}
