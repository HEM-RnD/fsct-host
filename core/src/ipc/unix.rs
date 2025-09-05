// Platform-specific IPC transport for Unix using Tokio UnixListener/UnixStream
// Provides identical API to windows.rs: EndpointListenerBuilder, EndpointListener, EndpointClient

use anyhow::{Context, Result};
use futures::Stream;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{UnixListener, UnixStream};

use std::os::fd::OwnedFd;

// Placeholder security attributes API to keep parity with windows implementation.
// On Unix, we only allow setting file mode (permissions) when bound from path.
#[derive(Clone, Default)]
pub struct SecurityAttributesUnix {
    pub mode: Option<u32>, // e.g., 0o666
}

pub struct EndpointListenerBuilder {
    kind: EndpointKind,
    attrs: SecurityAttributesUnix,
}

enum EndpointKind {
    FromPath(String),
    FromFd(OwnedFd),
}

impl EndpointListenerBuilder {
    pub fn from_path(path: String) -> Self {
        Self { kind: EndpointKind::FromPath(path), attrs: SecurityAttributesUnix::default() }
    }
    pub fn from_fd(fd: OwnedFd) -> Self {
        Self { kind: EndpointKind::FromFd(fd), attrs: SecurityAttributesUnix::default() }
    }
    pub fn security_attributes(mut self, attrs: SecurityAttributesUnix) -> Self {
        self.attrs = attrs;
        self
    }

    pub async fn build(self) -> Result<EndpointListener> {
        match self.kind {
            EndpointKind::FromPath(path) => {
                // Ensure parent directory and remove stale socket
                if let Some(parent) = Path::new(&path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::remove_file(&path);
                let listener = UnixListener::bind(&path)
                    .with_context(|| format!("failed to bind unix socket at {}", path))?;
                if let Some(mode) = self.attrs.mode {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let mut perm = meta.permissions();
                        perm.set_mode(mode);
                        let _ = std::fs::set_permissions(&path, perm);
                    }
                }
                Ok(EndpointListener { inner: ListenerInner::Unix(listener) })
            }
            EndpointKind::FromFd(fd) => {
                let std_listener = std::os::unix::net::UnixListener::from(fd);
                std_listener
                    .set_nonblocking(true)
                    .context("failed to set nonblocking on fd")?;
                let listener = UnixListener::from_std(std_listener)
                    .context("failed to adopt fd as UnixListener")?;
                Ok(EndpointListener { inner: ListenerInner::Unix(listener) })
            }
        }
    }
}

pub struct EndpointListener {
    inner: ListenerInner,
}

enum ListenerInner {
    Unix(UnixListener),
}

impl EndpointListener {
    pub fn listen(self) -> EndpointIncoming {
        EndpointIncoming { inner: self.inner }
    }
}

pub struct EndpointIncoming {
    inner: ListenerInner,
}

impl Stream for EndpointIncoming {
    type Item = Result<UnixStream>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match &mut this.inner {
            ListenerInner::Unix(listener) => match listener.poll_accept(cx) {
                Poll::Ready(Ok((stream, _addr))) => Poll::Ready(Some(Ok(stream))),
                Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e.into()))),
                Poll::Pending => Poll::Pending,
            },
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
