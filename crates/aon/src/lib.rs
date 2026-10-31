//! Aonin's audio engine: device I/O, the UDP sender and hub, discovery, and
//! the host/client roles the desktop app drives.

pub mod audio;
pub mod discovery;
pub mod engine;
pub mod net;
#[cfg(windows)]
mod winloop;
