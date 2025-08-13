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

use std::future::Future;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// A handle passed to background tasks that lets them observe a stop/shutdown request.
///
/// It wraps a oneshot Receiver and provides a mutable reference for use in select! statements.
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct StopHandle {
    shutdown_rx: oneshot::Receiver<()>,
}

impl StopHandle {
    /// Internal constructor from a receiver
    fn new(shutdown_rx: oneshot::Receiver<()>) -> Self { Self { shutdown_rx } }

    /// Awaits a signal from the shutdown receiver.
    ///
    /// This asynchronous function listens for a signal on the `shutdown_rx` channel.
    /// When a signal is received, the function resolves. If the receiver encounters
    /// an error or is closed, it will default to an empty result.
    ///
    /// # Behavior
    /// - If a signal is successfully received, the function completes.
    /// - If the receiver is dropped or an error occurs, the function will resolve using `unwrap_or_default()`.
    ///
    /// # Usage
    /// This function is typically used to handle graceful shutdown signals for long-running tasks.
    ///
    /// # Example
    /// ```rust
    /// use fsct_core::spawn_service;
    ///
    /// async fn run_service () {
    ///     let service_handle = spawn_service(move |mut stop_handle| async move {
    ///         // Wait for shutdown signal
    ///         println!("Waiting for shutdown signal...");
    ///         stop_handle.signaled().await;
    ///         println!("Shutdown signal received!");
    ///     });
    ///     // Trigger the shutdown
    ///     service_handle.shutdown().await.unwrap();
    /// }
    /// ```
    pub async fn signaled(&mut self) {
        (&mut self.shutdown_rx).await.unwrap_or_default();
    }
}

/// A unified handle for background service tasks that support cooperative shutdown and abort.
pub struct ServiceHandle {
    join: JoinHandle<()>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl ServiceHandle {
    /// Construct a new ServiceHandle from a spawned task handle and a oneshot shutdown sender.
    pub fn new(join: JoinHandle<()>, shutdown_tx: oneshot::Sender<()>) -> Self {
        Self { join, shutdown_tx: Some(shutdown_tx) }
    }

    /// Request cooperative shutdown signal without awaiting task completion.
    pub fn request_shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Await task completion without sending a shutdown signal.
    pub async fn await_join(self) -> Result<(), tokio::task::JoinError> {
        self.join.await
    }

    /// Request cooperative shutdown and await task completion.
    pub async fn shutdown(mut self) -> Result<(), tokio::task::JoinError> {
        self.request_shutdown();
        self.await_join().await
    }

    /// Forcefully abort the underlying task.
    pub fn abort(self) {
        self.join.abort();
    }
}

/// Spawn a background service task with a standard stop mechanism.
///
/// The provided function will receive a StopHandle to await for shutdown, and will be executed
/// on a Tokio task. The returned ServiceHandle allows triggering a cooperative shutdown or aborting.
pub fn spawn_service<Fut, Func>(f: Func) -> ServiceHandle
where
    Fut: Future<Output=()> + Send + 'static,
    Func: FnOnce(StopHandle) -> Fut + Send + 'static,
{
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let stop = StopHandle::new(shutdown_rx);
    let join = tokio::spawn(async move {
        f(stop).await;
    });
    ServiceHandle::new(join, shutdown_tx)
}

/// A container for multiple ServiceHandles with a single shutdown method.
pub struct MultiServiceHandle {
    handles: Vec<ServiceHandle>,
}

impl Default for MultiServiceHandle {
    fn default() -> Self { Self { handles: Vec::new() } }
}

impl MultiServiceHandle {
    /// Create an empty MultiServiceHandle
    pub fn new() -> Self { Self::default() }

    /// Create with reserved capacity
    pub fn with_capacity(cap: usize) -> Self { Self { handles: Vec::with_capacity(cap) } }

    /// Add a ServiceHandle to be managed
    pub fn add(&mut self, handle: ServiceHandle) { self.handles.push(handle); }

    /// Number of contained handles
    pub fn len(&self) -> usize { self.handles.len() }

    /// Whether there are no handles
    pub fn is_empty(&self) -> bool { self.handles.is_empty() }

    /// Request shutdown for all services, then await their completion.
    /// Returns Ok(()) if all joins succeed; otherwise returns the first JoinError encountered.
    pub async fn shutdown(mut self) -> Result<(), tokio::task::JoinError> {
        // First, request shutdown on all
        for h in &mut self.handles {
            h.request_shutdown();
        }
        // Then, await all joins, capturing first error if any
        let mut first_err: Option<tokio::task::JoinError> = None;
        for h in self.handles.into_iter() {
            if let Err(e) = h.await_join().await {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(())
        }
    }
}
