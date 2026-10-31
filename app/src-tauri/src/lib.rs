//! Aonin desktop shell: a tray (Windows) / menu bar (macOS) icon that opens
//! a popover, plus a main window. Closing the window keeps Aonin running in
//! the tray; Quit lives in the popover.

use aon::engine::{self, DeviceRole, Engine, Prefs};
use aon_core::protocol::Source;
use serde::{Deserialize, Serialize};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_positioner::{Position, WindowExt};

const POPOVER: &str = "popover";
const MAIN: &str = "main";
const TRAY: &str = "aonin";

/// Persisted between launches in the app config dir.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
struct Config {
    /// Random, stable identity of this device on the network.
    device: u64,
    last_host: Option<u64>,
    reconnect: Option<bool>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    system_source: Option<String>,
    #[serde(default)]
    mic: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    quiet_wifi: bool,
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("config.json"))
}

fn load_config(app: &AppHandle) -> Config {
    let mut cfg: Config = config_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if cfg.device == 0 {
        cfg.device = RandomState::new().hash_one(Instant::now()) | 1;
        save_config(app, &cfg);
    }
    cfg
}

fn save_config(app: &AppHandle, cfg: &Config) {
    if let Some(p) = config_path(app) {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, serde_json::to_vec_pretty(cfg).unwrap_or_default());
    }
}

type Eng<'a> = State<'a, Arc<Engine>>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn parse_host(id: &str) -> Result<u64, String> {
    engine::parse_id(id).ok_or_else(|| "unknown host".to_string())
}

#[tauri::command(async)]
fn become_host(engine: Eng) -> Result<(), String> {
    engine.become_host().map_err(err)
}

#[tauri::command(async)]
fn stop_hosting(engine: Eng) {
    engine.stop_hosting();
}

#[tauri::command(async)]
fn connect(engine: Eng, host_id: String) -> Result<(), String> {
    engine.connect(parse_host(&host_id)?).map_err(err)
}

#[tauri::command(async)]
fn disconnect(engine: Eng) {
    engine.forget_host();
}

#[tauri::command]
fn set_volume(engine: Eng, client: String, volume: f32) {
    engine.set_volume(&client, volume);
}

#[tauri::command]
fn set_muted(engine: Eng, client: String, muted: bool) {
    engine.set_muted(&client, muted);
}

#[tauri::command(async)]
fn set_send(engine: Eng, source: String, on: bool) {
    engine.set_send(if source == "mic" { Source::Mic } else { Source::System }, on);
}

/// Ask the host to stream its own audio ("system") or mic back to us.
#[tauri::command(async)]
fn set_hear(engine: Eng, source: String, on: bool) -> Result<(), String> {
    engine.set_hear(if source == "mic" { Source::Mic } else { Source::System }, on).map_err(err)
}

#[tauri::command]
fn set_start_at_login(app: AppHandle, engine: Eng, on: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    if on { launcher.enable() } else { launcher.disable() }.map_err(err)?;
    engine.prefs().start_at_login = launcher.is_enabled().unwrap_or(on);
    Ok(())
}

/// macOS: AirDrop/Handoff (AWDL) makes Wi-Fi stall every few seconds while
/// it scans. Turning the awdl0 interface off needs an administrator, so macOS
/// shows its own password prompt. It stays as set until switched back or the
/// Mac restarts.
#[tauri::command]
fn set_quiet_wifi(app: AppHandle, engine: Eng, on: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let cmd = if on { "ifconfig awdl0 down" } else { "ifconfig awdl0 up" };
        let script = format!("do shell script \"{cmd}\" with administrator privileges");
        let status = std::process::Command::new("osascript").args(["-e", &script]).status().map_err(err)?;
        if !status.success() {
            return Err("Wi-Fi setting not changed (the password prompt was cancelled).".into());
        }
    }
    #[cfg(not(target_os = "macos"))]
    if on {
        return Err("Only needed on macOS.".into());
    }
    engine.prefs().quiet_wifi = on;
    let mut cfg = load_config(&app);
    cfg.quiet_wifi = on;
    save_config(&app, &cfg);
    Ok(())
}

/// Lossless, CD, Balanced or Saver for what this device sends.
#[tauri::command(async)]
fn set_quality(app: AppHandle, engine: Eng, quality: String) -> Result<(), String> {
    let q = aon_core::protocol::Quality::from_name(&quality).ok_or("unknown quality")?;
    engine.set_quality(q);
    let mut cfg = load_config(&app);
    cfg.quality = Some(quality);
    save_config(&app, &cfg);
    Ok(())
}

/// Choose a device by name; an empty name follows the system default.
#[tauri::command(async)]
fn set_device(app: AppHandle, engine: Eng, role: String, name: String) {
    let name = (!name.is_empty()).then_some(name);
    let role = match role.as_str() {
        "output" => DeviceRole::Output,
        "mic" => DeviceRole::Mic,
        _ => DeviceRole::SystemSource,
    };
    engine.set_device(role, name.clone());
    let mut cfg = load_config(&app);
    match role {
        DeviceRole::Output => cfg.output = name,
        DeviceRole::SystemSource => cfg.system_source = name,
        DeviceRole::Mic => cfg.mic = name,
    }
    save_config(&app, &cfg);
}

#[tauri::command]
fn set_reconnect(app: AppHandle, engine: Eng, on: bool) {
    engine.set_reconnect(on);
    let mut cfg = load_config(&app);
    cfg.reconnect = Some(on);
    save_config(&app, &cfg);
}

/// Push the engine snapshot to both windows ~10 times a second and persist
/// the last host whenever it changes.
fn start_poller(app: AppHandle, engine: Arc<Engine>, mut cfg: Config) {
    std::thread::Builder::new()
        .name("aonin-ui".into())
        .spawn(move || loop {
            let snap = engine.tick();
            let _ = app.emit("state", &snap);
            if engine.last_host() != cfg.last_host {
                // Reload first: other settings may have been saved meanwhile.
                cfg = load_config(&app);
                cfg.last_host = engine.last_host();
                save_config(&app, &cfg);
            }
            std::thread::sleep(Duration::from_millis(100));
        })
        .expect("spawn poller");
}

/// Tray icon for the current state. macOS gets a template image (black +
/// alpha) that the system tints for light and dark menu bars.
fn tray_image(active: bool) -> Image<'static> {
    #[cfg(target_os = "macos")]
    let bytes: &[u8] = if active {
        include_bytes!("../icons/tray/menubar-active@2x.png")
    } else {
        include_bytes!("../icons/tray/menubar-idle@2x.png")
    };
    #[cfg(not(target_os = "macos"))]
    let bytes: &[u8] = if active { include_bytes!("../icons/tray/active.png") } else { include_bytes!("../icons/tray/idle.png") };
    Image::from_bytes(bytes).expect("bundled tray icon is a valid PNG")
}

fn build_popover(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(app, POPOVER, WebviewUrl::App("popover".into()))
        .title("Aonin")
        .inner_size(360.0, 320.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .shadow(true);
    // Windows 11 rounds and shadows undecorated windows itself; macOS needs a
    // transparent window so the page can draw its own rounded housing.
    #[cfg(target_os = "macos")]
    let builder = builder.transparent(true);
    builder.build()
}

fn build_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Aonin")
        .inner_size(860.0, 600.0)
        .min_inner_size(720.0, 480.0)
        .visible(false)
        .build()
}

fn toggle_popover(app: &AppHandle) {
    let Some(pop) = app.get_webview_window(POPOVER) else { return };
    if pop.is_visible().unwrap_or(false) {
        let _ = pop.hide();
        return;
    }
    #[cfg(target_os = "macos")]
    let at = Position::TrayBottomCenter;
    #[cfg(not(target_os = "macos"))]
    let at = Position::TrayCenter;
    let _ = pop.move_window(at);
    let _ = pop.show();
    let _ = pop.set_focus();
}

fn on_tray_event(tray: &TrayIcon, event: TrayIconEvent) {
    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
        toggle_popover(tray.app_handle());
    }
}

/// Show the main window on a section (devices, audio, network, advanced).
#[tauri::command]
fn open_window(app: AppHandle, section: String) -> Result<(), String> {
    let win = app.get_webview_window(MAIN).ok_or("main window missing")?;
    if let Some(pop) = app.get_webview_window(POPOVER) {
        let _ = pop.hide();
    }
    let section: String = section.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    win.eval(format!("location.hash = '{section}'")).map_err(|e| e.to_string())?;
    win.show().map_err(|e| e.to_string())?;
    win.unminimize().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())
}

/// Swap the tray icon between idle and active (hosting or connected).
#[tauri::command]
fn set_tray_active(app: AppHandle, active: bool) -> Result<(), String> {
    let tray = app.tray_by_id(TRAY).ok_or("tray missing")?;
    tray.set_icon(Some(tray_image(active))).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    tray.set_icon_as_template(true).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn quit(app: AppHandle, engine: Eng) {
    engine.shutdown();
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let handle = app.handle();
            let cfg = load_config(handle);
            let name = gethostname::gethostname().to_string_lossy().into_owned();
            let prefs = Prefs {
                reconnect: cfg.reconnect.unwrap_or(true),
                last_host: cfg.last_host,
                output: cfg.output.clone(),
                system_source: cfg.system_source.clone(),
                mic: cfg.mic.clone(),
                quality: cfg.quality.as_deref().and_then(aon_core::protocol::Quality::from_name).unwrap_or_default(),
                start_at_login: app.autolaunch().is_enabled().unwrap_or(false),
                quiet_wifi: cfg.quiet_wifi,
            };
            let engine = Engine::new(cfg.device, name, prefs);
            app.manage(engine.clone());
            start_poller(handle.clone(), engine, cfg);
            build_popover(handle)?;
            build_main(handle)?;
            TrayIconBuilder::with_id(TRAY)
                .icon(tray_image(false))
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("Aonin")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(on_tray_event)
                .build(app)?;
            // A tray app: no Dock icon on macOS.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Clicking elsewhere dismisses the popover.
            WindowEvent::Focused(false) if window.label() == POPOVER => {
                let _ = window.hide();
            }
            // Closing the main window keeps Aonin running in the tray.
            WindowEvent::CloseRequested { api, .. } if window.label() == MAIN => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            open_window,
            set_tray_active,
            quit,
            become_host,
            stop_hosting,
            connect,
            disconnect,
            set_volume,
            set_muted,
            set_send,
            set_hear,
            set_quality,
            set_start_at_login,
            set_quiet_wifi,
            set_device,
            set_reconnect
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
