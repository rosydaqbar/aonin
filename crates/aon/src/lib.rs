//! Aonin's audio engine: device I/O, the UDP sender and hub, discovery, and
//! the host/client roles the desktop app drives.

pub mod audio;
pub mod discovery;
pub mod engine;
pub mod net;
pub mod permissions;
pub mod private;
#[cfg(target_os = "macos")]
mod macloop;
#[cfg(windows)]
mod winloop;
#[cfg(windows)]
mod speakers;

/// If this process was started as Aonin's speaker guard (Windows), run it
/// and exit. Call first thing in `main`.
pub fn run_speaker_guard_if_asked() {
    #[cfg(windows)]
    speakers::run_guard_if_asked();
}
