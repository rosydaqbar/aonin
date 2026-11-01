//! What the operating system is stopping Aonin from doing, checked in the
//! background so the UI can say so and offer the fix.
//!
//! - macOS: recording system audio and using the microphone (privacy
//!   permissions, read through TCC's preflight check). Local Network access
//!   has no such check; the engine notices refused packets instead.
//! - Windows: an inbound Windows Firewall rule for Aonin, and the microphone
//!   privacy switches.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The latest results; `false` means fine or unknown.
#[derive(Clone, Copy, Debug, Default)]
pub struct Found {
    pub audio_capture_denied: bool,
    pub mic_denied: bool,
    pub firewall_missing: bool,
}

/// Re-checks every few seconds on its own thread.
pub struct Checker {
    found: Arc<Mutex<Found>>,
    now: Arc<AtomicBool>,
}

const EVERY: Duration = Duration::from_secs(3);

impl Checker {
    pub fn start() -> Self {
        let found = Arc::new(Mutex::new(Found::default()));
        let now = Arc::new(AtomicBool::new(false));
        let (f, n) = (found.clone(), now.clone());
        std::thread::Builder::new()
            .name("aonin-permissions".into())
            .spawn(move || loop {
                *f.lock().unwrap() = check();
                // Sleep in short steps so a fix is picked up right away.
                for _ in 0..(EVERY.as_millis() / 100) {
                    if n.swap(false, Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })
            .expect("spawn permission checker");
        Self { found, now }
    }

    pub fn get(&self) -> Found {
        *self.found.lock().unwrap()
    }

    /// Check again without waiting for the next round.
    pub fn recheck(&self) {
        self.now.store(true, Relaxed);
    }
}

fn check() -> Found {
    #[cfg(target_os = "macos")]
    return Found { audio_capture_denied: mac::denied("kTCCServiceAudioCapture"), mic_denied: mac::denied("kTCCServiceMicrophone"), firewall_missing: false };
    #[cfg(windows)]
    return Found { audio_capture_denied: false, mic_denied: win::mic_denied(), firewall_missing: win::firewall_missing() };
    #[cfg(not(any(target_os = "macos", windows)))]
    Found::default()
}

/// Act on an issue the UI shows: open the right settings page, or (Windows
/// Firewall) add the rule after the administrator prompt.
pub fn fix(issue: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let pane = match issue {
            "localNetwork" => "Privacy_LocalNetwork",
            "audioCapture" => "Privacy_ScreenCapture",
            "microphone" => "Privacy_Microphone",
            _ => anyhow::bail!("unknown issue {issue}"),
        };
        std::process::Command::new("open").arg(format!("x-apple.systempreferences:com.apple.preference.security?{pane}")).status()?;
        Ok(())
    }
    #[cfg(windows)]
    match issue {
        "microphone" => win::open("ms-settings:privacy-microphone"),
        "firewall" => win::allow_in_firewall(),
        _ => anyhow::bail!("unknown issue {issue}"),
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    anyhow::bail!("unknown issue {issue}")
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_core_foundation::CFString;
    use std::ffi::{c_char, c_int, c_void};
    use std::sync::OnceLock;

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    /// `TCCAccessPreflight(service, options)`: 0 allowed, 1 denied, other
    /// values not decided yet. TCC is a private framework, so it is looked up
    /// at run time and the check is skipped if it ever goes away.
    type Preflight = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;

    fn preflight() -> Option<Preflight> {
        static FN: OnceLock<Option<Preflight>> = OnceLock::new();
        *FN.get_or_init(|| {
            // Safety: dlopen/dlsym with NUL-terminated names; the symbol has
            // the signature above.
            unsafe {
                let lib = dlopen(c"/System/Library/PrivateFrameworks/TCC.framework/TCC".as_ptr(), 1);
                if lib.is_null() {
                    return None;
                }
                let sym = dlsym(lib, c"TCCAccessPreflight".as_ptr());
                (!sym.is_null()).then(|| std::mem::transmute::<*mut c_void, Preflight>(sym))
            }
        })
    }

    /// The user turned this permission off for Aonin.
    pub fn denied(service: &str) -> bool {
        let Some(f) = preflight() else { return false };
        let name = CFString::from_str(service);
        // Safety: a live CFString and no options.
        unsafe { f(&*name as *const CFString as *const c_void, std::ptr::null()) == 1 }
    }
}

#[cfg(windows)]
mod win {
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwPolicy2, INetFwRule, NET_FW_ACTION_ALLOW, NET_FW_PROFILE2_DOMAIN, NET_FW_PROFILE2_PRIVATE, NET_FW_PROFILE2_PUBLIC,
        NET_FW_RULE_DIR_IN, NetFwPolicy2,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::{BSTR, HSTRING};

    const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

    fn reg_says_deny(root: HKEY, path: &str) -> bool {
        let mut buf = [0u16; 16];
        let mut size = (buf.len() * 2) as u32;
        // Safety: the buffer and its size in bytes are passed together.
        let status = unsafe { RegGetValueW(root, &HSTRING::from(path), &HSTRING::from("Value"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) };
        status.is_ok() && String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]).eq_ignore_ascii_case("Deny")
    }

    /// Microphone access is off for this device, for apps, or for desktop apps.
    pub fn mic_denied() -> bool {
        reg_says_deny(HKEY_LOCAL_MACHINE, &CONSENT.replace("Software", "SOFTWARE"))
            || reg_says_deny(HKEY_CURRENT_USER, CONSENT)
            || reg_says_deny(HKEY_CURRENT_USER, &format!(r"{CONSENT}\NonPackaged"))
    }

    /// The firewall is on and no enabled inbound allow rule named "Aonin"
    /// covers this executable (the installer adds one; so does Windows when
    /// the user allows Aonin at its prompt).
    pub fn firewall_missing() -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        let exe = exe.to_string_lossy().to_lowercase();
        // Safety: plain COM calls on this thread.
        let result = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            (|| -> windows::core::Result<bool> {
                let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)?;
                let current = policy.CurrentProfileTypes()?;
                let mut on = false;
                for p in [NET_FW_PROFILE2_DOMAIN, NET_FW_PROFILE2_PRIVATE, NET_FW_PROFILE2_PUBLIC] {
                    if current & p.0 != 0 && policy.get_FirewallEnabled(p)?.as_bool() {
                        on = true;
                    }
                }
                if !on {
                    return Ok(false);
                }
                let Ok(rule): windows::core::Result<INetFwRule> = policy.Rules()?.Item(&BSTR::from("Aonin")) else {
                    return Ok(true);
                };
                let allows = rule.Enabled()?.as_bool()
                    && rule.Action()? == NET_FW_ACTION_ALLOW
                    && rule.Direction()? == NET_FW_RULE_DIR_IN
                    && rule.ApplicationName()?.to_string().to_lowercase() == exe;
                Ok(!allows)
            })()
        };
        result.unwrap_or(false)
    }

    pub fn open(uri: &str) -> anyhow::Result<()> {
        std::process::Command::new("explorer").arg(uri).status()?;
        Ok(())
    }

    /// Replace any "Aonin" rules for this executable (including a block rule
    /// left by a cancelled prompt) with one allow rule, after the
    /// administrator prompt.
    pub fn allow_in_firewall() -> anyhow::Result<()> {
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe()?.to_string_lossy().into_owned();
        let cmd = format!(
            "netsh advfirewall firewall delete rule name=Aonin program=\"{exe}\" & netsh advfirewall firewall add rule name=Aonin dir=in action=allow program=\"{exe}\" enable=yes profile=any"
        );
        let script = format!("Start-Process cmd -Verb RunAs -WindowStyle Hidden -Wait -ArgumentList '/c {}'", cmd.replace('\'', "''"));
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()?;
        anyhow::ensure!(status.success(), "the administrator prompt was cancelled");
        Ok(())
    }
}
