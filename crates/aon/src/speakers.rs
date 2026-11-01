//! Silencing this PC's speakers while it sends its audio (Windows).
//!
//! Process loopback records what apps play before the endpoint's mute is
//! applied, so muting the speakers leaves the stream to the host untouched.
//!
//! The speakers must never stay muted after Aonin is gone, however it ends.
//! A crash or "End task" runs no code in Aonin, so muting also starts a small
//! guard: a detached copy of this executable that waits for Aonin's process
//! to exit and unmutes the moment it does. It is not in Aonin's process tree,
//! so ending that tree leaves the guard running. A marker file names the
//! muted endpoint; the guard stops as soon as the marker is gone (a normal
//! unmute), and on launch Aonin clears a marker left by a power cut.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::core::HSTRING;

/// First argument that turns any Aonin executable into the guard.
const GUARD_ARG: &str = "--aonin-speaker-guard";
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

fn marker() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Aonin").join("speakers-muted"))
}

/// The marker holds the endpoint ID and the token of the mute that wrote it.
fn read_marker() -> Option<(String, String)> {
    let text = std::fs::read_to_string(marker()?).ok()?;
    let mut lines = text.lines();
    let id = lines.next()?.trim().to_string();
    Some((id, lines.next().unwrap_or("").trim().to_string()))
}

fn with_com<T>(f: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
    // Safety: balanced COM init on this thread; every COM object is dropped
    // inside `f`, before the uninit.
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = f();
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

/// The endpoint's ID and volume control; `None` is the default output.
fn endpoint(id: Option<&str>) -> windows::core::Result<(String, IAudioEndpointVolume)> {
    // Safety: plain COM calls; the ID string is freed after copying.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device: IMMDevice = match id {
            Some(id) => devices.GetDevice(&HSTRING::from(id))?,
            None => devices.GetDefaultAudioEndpoint(eRender, eConsole)?,
        };
        let raw = device.GetId()?;
        let id = raw.to_string();
        CoTaskMemFree(Some(raw.0 as _));
        Ok((id?, device.Activate(CLSCTX_ALL, None)?))
    }
}

fn unmute(id: &str) {
    // Safety: see `endpoint`.
    if let Err(e) = with_com(|| unsafe { endpoint(Some(id))?.1.SetMute(false, std::ptr::null()) }) {
        eprintln!("unmuting the speakers failed: {e}");
    }
}

/// Unmute whatever the marker names and remove it.
fn clear_marker() {
    if let Some((id, _)) = read_marker() {
        unmute(&id);
    }
    if let Some(m) = marker() {
        let _ = std::fs::remove_file(m);
    }
}

/// Speakers Aonin muted; dropping it unmutes them.
pub struct Muted {
    id: String,
}

impl Drop for Muted {
    fn drop(&mut self) {
        // Marker first: the guard then exits instead of unmuting again.
        if let Some(m) = marker() {
            let _ = std::fs::remove_file(m);
        }
        unmute(&self.id);
    }
}

/// Mute an output endpoint (`None` = the default). Speakers the user had
/// already muted are left alone and stay muted afterwards.
pub fn mute(id: Option<&str>) -> Option<Muted> {
    let token = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).to_string();
    // Safety: see `endpoint`.
    let result = with_com(|| unsafe {
        let (id, volume) = endpoint(id)?;
        if volume.GetMute()?.as_bool() {
            return Ok(None);
        }
        // Marker and guard come before the mute, so no moment is unguarded.
        let Some(m) = marker() else { return Ok(None) };
        let _ = std::fs::create_dir_all(m.parent().unwrap());
        if std::fs::write(&m, format!("{id}\n{token}")).is_err() || !spawn_guard(&token) {
            let _ = std::fs::remove_file(&m);
            eprintln!("speaker guard unavailable; leaving the speakers on");
            return Ok(None);
        }
        volume.SetMute(true, std::ptr::null())?;
        Ok(Some(Muted { id }))
    });
    result.unwrap_or_else(|e| {
        eprintln!("muting the speakers failed: {e}");
        if let Some(m) = marker() {
            let _ = std::fs::remove_file(m);
        }
        None
    })
}

/// Start the guard through a short-lived middle process, so the guard ends up
/// outside Aonin's process tree.
fn spawn_guard(token: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    std::process::Command::new(exe)
        .args([GUARD_ARG, "spawn", &std::process::id().to_string(), token])
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .status()
        .is_ok_and(|s| s.success())
}

/// Unmute speakers a previous run left muted (it ended before it could, and
/// its guard didn't get to either, e.g. after a power cut).
pub fn recover() {
    clear_marker();
}

/// If this process was started as the speaker guard, do that job and exit.
/// Every Aonin executable calls this first thing in `main`.
pub fn run_guard_if_asked() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(GUARD_ARG) || args.len() < 5 {
        return;
    }
    let (stage, pid, token) = (args[2].as_str(), &args[3], &args[4]);
    if stage == "spawn" {
        // The middle process: start the real guard and exit, orphaning it.
        let ok = std::env::current_exe().is_ok_and(|exe| {
            std::process::Command::new(exe)
                .args([GUARD_ARG, "watch", pid, token])
                .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
                .spawn()
                .is_ok()
        });
        std::process::exit(if ok { 0 } else { 1 });
    }
    let Ok(pid) = pid.parse::<u32>() else { std::process::exit(1) };
    // Safety: a handle used only to wait on, closed before exiting.
    unsafe {
        let Ok(aonin) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
            // Aonin is already gone: unmute if the mute is still ours.
            if read_marker().is_some_and(|(_, t)| &t == token) {
                clear_marker();
            }
            std::process::exit(0);
        };
        loop {
            let still_ours = read_marker().is_some_and(|(_, t)| &t == token);
            if !still_ours {
                break; // unmuted normally, or a newer mute has its own guard
            }
            if WaitForSingleObject(aonin, 250) != WAIT_TIMEOUT {
                // Aonin exited (or the wait failed): never leave them muted.
                if read_marker().is_some_and(|(_, t)| &t == token) {
                    clear_marker();
                }
                break;
            }
        }
        let _ = CloseHandle(aonin);
    }
    std::process::exit(0);
}
