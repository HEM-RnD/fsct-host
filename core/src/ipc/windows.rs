// Platform-specific IPC transport for Windows using Tokio Named Pipes
// Provides EndpointListenerBuilder, EndpointListener (as Stream), EndpointClient

use anyhow::{anyhow, Context, Result};
use futures::Stream;
use futures::stream;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};

#[derive(Clone, Default)]
pub struct SecurityAttributesWindows {
    // Placeholder for future security settings; for now accept everyone by default.
    pub allow_everyone_connect: bool,
}

pub struct EndpointListenerBuilder {
    name: String,
    attrs: SecurityAttributesWindows,
}

impl EndpointListenerBuilder {
    pub fn from_path(name: String) -> Self { // name is like \\.\pipe\fsct...
        Self { name, attrs: SecurityAttributesWindows { allow_everyone_connect: true } }
    }
    pub fn security_attributes(mut self, attrs: SecurityAttributesWindows) -> Self {
        self.attrs = attrs;
        self
    }
    pub async fn build(self) -> Result<EndpointListener> {
        // Defer server instance creation to listen() unfold
        Ok(EndpointListener { name: self.name, attrs: self.attrs })
    }
}

fn create_server(name: &str, _attrs: &SecurityAttributesWindows) -> Result<NamedPipeServer> {
    // Security: default DACL of process; allowing everyone connect typically requires explicit descriptor,
    // but for tests and local usage, default is sufficient. If needed, one could use security_descriptor on ServerOptions.
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(name)
        .with_context(|| format!("failed to create named pipe server for {}", name))?;
    Ok(server)
}

pub struct EndpointListener {
    name: String,
    attrs: SecurityAttributesWindows,
}

impl EndpointListener {
    pub fn listen(self) -> impl Stream<Item = Result<NamedPipeServer>> + Send {
        let name = self.name.clone();
        let attrs = self.attrs.clone();
        stream::unfold((name, attrs), |(name, attrs)| async move {
            // Create a server instance and wait for a client to connect
            let server = match create_server(&name, &attrs) {
                Ok(s) => s,
                Err(e) => return Some((Err(e), (name, attrs))),
            };
            match server.connect().await {
                Ok(()) => Some((Ok(server), (name, attrs))),
                Err(e) => Some((Err(anyhow!(e)), (name, attrs))),
            }
        })
    }
}

pub struct EndpointClient;

impl EndpointClient {
    pub async fn connect(name: String) -> Result<NamedPipeClient> {
        ClientOptions::new().open(name.as_str()).context("windows pipe connect failed")
    }
}

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
