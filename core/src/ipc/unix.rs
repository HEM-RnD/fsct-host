// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

// Platform-specific IPC transport for Unix using Tokio UnixListener/UnixStream
// Simplified: no public security attributes; open to all by default. Listener provides from_path/from_fd.

use anyhow::{Context, Result};
use futures::Stream;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{UnixListener, UnixStream};

use std::os::fd::OwnedFd;

pub struct EndpointListener {
    inner: UnixListener,
}

impl EndpointListener {
    /// Bind a Unix socket at the given path. Ensures parent dir exists and sets 0o666 perms.
    pub async fn from_path(path: String) -> Result<Self> {
        // Ensure parent directory and remove stale socket
        if let Some(parent) = Path::new(&path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::remove_file(&path).await;

        let listener = UnixListener::bind(&path)
            .with_context(|| format!("failed to bind unix socket at {}", path))?;

        // Set mode 0o666 (rw for everyone)
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = tokio::fs::metadata(&path).await {
            let mut perm = meta.permissions();
            perm.set_mode(0o666);
            let _ = tokio::fs::set_permissions(&path, perm).await;
        }

        Ok(EndpointListener { inner: listener })
    }

    /// Adopt an existing file descriptor as a non-blocking UnixListener (Unix only).
    pub fn from_fd(fd: OwnedFd) -> Result<Self> {
        let std_listener = std::os::unix::net::UnixListener::from(fd);
        std_listener
            .set_nonblocking(true)
            .context("failed to set nonblocking on fd")?;
        let listener = UnixListener::from_std(std_listener)
            .context("failed to adopt fd as UnixListener")?;
        Ok(EndpointListener { inner: listener })
    }

    pub fn listen(self) -> Result<EndpointIncoming> {
        Ok(EndpointIncoming { inner: self.inner })
    }
}

pub struct EndpointIncoming {
    inner: UnixListener,
}

impl Stream for EndpointIncoming {
    type Item = Result<UnixStream>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.poll_accept(cx) {
            Poll::Ready(Ok((stream, _addr))) => Poll::Ready(Some(Ok(stream))),
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e.into()))),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct EndpointClient;

impl EndpointClient {
    pub async fn connect(path: String) -> Result<UnixStream> {
        UnixStream::connect(path).await.context("unix client connect failed")
    }
}

// Re-export traits needed by server/client code
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
