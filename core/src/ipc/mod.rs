#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

pub mod server;
pub mod client;

#[cfg(unix)]
use unix as transport;
#[cfg(windows)]
use windows as transport;