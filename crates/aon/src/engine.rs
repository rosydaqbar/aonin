//! The app-facing engine: host/client roles on top of the sender, hub and
//! discovery loops, plus the snapshot the UI renders.
//!
//! Audio streams (cpal) are not `Send` on every platform, so each role runs
//! in its own thread that opens its device and keeps the stream alive until
//! told to stop.

use crate::audio;
use crate::discovery::{Advert, Discovery};
use crate::net::{self, HubShared, Identity, SendConfig, SendShared};
use anyhow::{Result, anyhow};
use aon_core::protocol::{DEFAULT_PORT, Os, Source};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Above this the row is marked unstable.
const DEGRADED_MS: f32 = 60.0;
/// No Status from the host for this long means the link is down.
const LINK_TIMEOUT_MS: u32 = 3_000;

struct HostRun {
    shared: Arc<HubShared>,
    thread: JoinHandle<()>,
    error: Arc<Mutex<Option<String>>>,
}

struct SendRun {
    shared: Arc<SendShared>,
    thread: JoinHandle<()>,
    started: Instant,
}

struct LinkRun {
    host: u64,
    name: String,
    macos: bool,
    clients: u32,
    addr: SocketAddr,
    system: Option<SendRun>,
    mic: Option<SendRun>,
}

#[derive(Clone, Copy)]
struct Mix {
    volume: f32,
    muted: bool,
}

pub struct Prefs {
    pub reconnect: bool,
    pub last_host: Option<u64>,
}

pub struct Engine {
    me: Identity,
    disco: Arc<Discovery>,
    host: Mutex<Option<HostRun>>,
    link: Mutex<Option<LinkRun>>,
    mix: Mutex<HashMap<String, Mix>>,
    prefs: Mutex<Prefs>,
    want_mic: Mutex<bool>,
    started: Instant,
}

// ---- snapshot: mirrors `AppState` in app/src/lib/state.svelte.ts ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    me: Me,
    gate: Option<&'static str>,
    searching: bool,
    hosting: Option<Hosting>,
    link: Option<Link>,
    hosts: Vec<HostInfo>,
    quality: &'static str,
    output_lost: bool,
    stats: Vec<Stats>,
    devices: Devices,
    options: Options,
    /// Shown when hosting or discovery failed to start.
    error: Option<String>,
}

#[derive(Serialize)]
struct Me {
    name: String,
    os: &'static str,
}

#[derive(Serialize)]
struct Hosting {
    output: String,
    clients: Vec<ClientRow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientRow {
    id: String,
    name: String,
    os: &'static str,
    sends: Sends,
    mark: &'static str,
    latency_ms: f32,
    recovered: u32,
    level: f32,
    volume: f32,
    muted: bool,
}

#[derive(Serialize, Clone, Copy, Default)]
struct Sends {
    system: bool,
    mic: bool,
}

#[derive(Serialize, Clone)]
struct HostInfo {
    id: String,
    name: String,
    os: &'static str,
    clients: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocked: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Link {
    host: HostInfo,
    status: &'static str,
    latency_ms: f32,
    send: Sends,
    hear: Sends,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    name: String,
    buffer_ms: f32,
    target_ms: f32,
    recovered: u32,
    concealed: u32,
    dropouts: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Devices {
    outputs: Vec<String>,
    mics: Vec<String>,
    output: String,
    mic: String,
    system_source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    reconnect: bool,
    start_at_login: bool,
    quiet_wifi: bool,
}

fn os_name(os: Os) -> &'static str {
    match os {
        Os::Windows => "windows",
        Os::MacOs => "macos",
    }
}

fn id_str(id: u64) -> String {
    format!("{id:016x}")
}

pub fn parse_id(s: &str) -> Option<u64> {
    u64::from_str_radix(s, 16).ok()
}

/// Map a linear peak to the meter's 0..1 over a 60 dB range.
fn meter(peak: f32) -> f32 {
    if peak <= 0.0 { 0.0 } else { ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0) }
}

impl Engine {
    pub fn new(device: u64, name: String, prefs: Prefs) -> Arc<Self> {
        let os = if cfg!(target_os = "macos") { Os::MacOs } else { Os::Windows };
        let disco = Arc::new(Discovery::default());
        *disco.me.lock().unwrap() = Advert { id: device, name: name.clone(), macos: os == Os::MacOs, port: DEFAULT_PORT, ..Default::default() };
        let d = disco.clone();
        std::thread::Builder::new().name("aonin-discovery".into()).spawn(move || d.run()).expect("spawn discovery");
        Arc::new(Self {
            me: Identity { device, name, os },
            disco,
            host: Mutex::new(None),
            link: Mutex::new(None),
            mix: Mutex::new(HashMap::new()),
            prefs: Mutex::new(prefs),
            want_mic: Mutex::new(false),
            started: Instant::now(),
        })
    }

    pub fn last_host(&self) -> Option<u64> {
        self.prefs.lock().unwrap().last_host
    }

    pub fn set_reconnect(&self, on: bool) {
        self.prefs.lock().unwrap().reconnect = on;
    }

    pub fn become_host(&self) -> Result<()> {
        let mut host = self.host.lock().unwrap();
        if host.is_some() {
            return Ok(());
        }
        let shared = Arc::new(HubShared::default());
        let error = Arc::new(Mutex::new(None));
        let (s, e) = (shared.clone(), error.clone());
        let thread = std::thread::Builder::new().name("aonin-hub".into()).spawn(move || {
            let run = || -> Result<()> {
                let (_stream, tx, rate) = audio::start_output(None)?;
                net::run_hub(DEFAULT_PORT, tx, rate, &s, false)
            };
            if let Err(err) = run() {
                *e.lock().unwrap() = Some(format!("Couldn't start hosting: {err}"));
            }
        })?;
        *host = Some(HostRun { shared, thread, error });
        Ok(())
    }

    pub fn stop_hosting(&self) {
        if let Some(h) = self.host.lock().unwrap().take() {
            h.shared.stop.store(true, Relaxed);
            let _ = h.thread.join();
        }
    }

    fn start_send(&self, addr: SocketAddr, source: Source) -> SendRun {
        let shared = Arc::new(SendShared::default());
        let s = shared.clone();
        let who = self.me.clone();
        let thread = std::thread::Builder::new()
            .name(format!("aonin-send-{source:?}"))
            .spawn(move || {
                let kind = match source {
                    Source::System => audio::Source::System,
                    Source::Mic => audio::Source::Mic,
                };
                let run = || -> Result<()> {
                    let (_stream, cap, rate) = audio::start_capture(kind, None)?;
                    net::run_sender(SendConfig { hub: addr, who, source, drop_percent: 0, verbose: false }, cap, rate, &s)
                };
                if let Err(e) = run() {
                    eprintln!("sender {source:?}: {e}");
                }
            })
            .expect("spawn sender");
        SendRun { shared, thread, started: Instant::now() }
    }

    fn stop_send(run: Option<SendRun>) {
        if let Some(r) = run {
            r.shared.stop.store(true, Relaxed);
            let _ = r.thread.join();
        }
    }

    pub fn connect(&self, host: u64) -> Result<()> {
        let peer = self.disco.peers.lock().unwrap().get(&host).cloned().ok_or_else(|| anyhow!("That host is no longer on the network."))?;
        if !peer.advert.hosting {
            return Err(anyhow!("{} stopped hosting.", peer.advert.name));
        }
        if self.my_sources().contains(&host) {
            return Err(anyhow!("{} already sends its audio to this device.", peer.advert.name));
        }
        self.disconnect();
        let addr = SocketAddr::new(peer.ip, peer.advert.port);
        let mic = *self.want_mic.lock().unwrap();
        *self.link.lock().unwrap() = Some(LinkRun {
            host,
            name: peer.advert.name.clone(),
            macos: peer.advert.macos,
            clients: peer.advert.clients,
            addr,
            system: Some(self.start_send(addr, Source::System)),
            mic: mic.then(|| self.start_send(addr, Source::Mic)),
        });
        self.prefs.lock().unwrap().last_host = Some(host);
        Ok(())
    }

    pub fn disconnect(&self) {
        if let Some(l) = self.link.lock().unwrap().take() {
            Self::stop_send(l.system);
            Self::stop_send(l.mic);
        }
    }

    /// The user's explicit disconnect also stops auto-reconnect to that host.
    pub fn forget_host(&self) {
        self.disconnect();
        self.prefs.lock().unwrap().last_host = None;
    }

    pub fn set_send(&self, source: Source, on: bool) {
        if source == Source::Mic {
            *self.want_mic.lock().unwrap() = on;
        }
        let mut link = self.link.lock().unwrap();
        let Some(l) = link.as_mut() else { return };
        let slot = match source {
            Source::System => &mut l.system,
            Source::Mic => &mut l.mic,
        };
        if on && slot.is_none() {
            *slot = Some(self.start_send(l.addr, source));
        } else if !on {
            Self::stop_send(slot.take());
        }
    }

    pub fn set_volume(&self, client: &str, volume: f32) {
        self.mix.lock().unwrap().entry(client.into()).or_insert(Mix { volume: 1.0, muted: false }).volume = volume.clamp(0.0, 1.0);
    }

    pub fn set_muted(&self, client: &str, muted: bool) {
        self.mix.lock().unwrap().entry(client.into()).or_insert(Mix { volume: 1.0, muted: false }).muted = muted;
    }

    /// Devices whose audio reaches this one: our clients, and everything
    /// that reaches them when they host too.
    fn my_sources(&self) -> HashSet<u64> {
        let peers = self.disco.peers.lock().unwrap();
        let mut out = HashSet::new();
        let mut todo: Vec<u64> = match self.host.lock().unwrap().as_ref() {
            Some(h) => h.shared.streams.lock().unwrap().iter().filter_map(|s| s.who.as_ref().map(|(w, _)| w.device)).collect(),
            None => Vec::new(),
        };
        while let Some(id) = todo.pop() {
            if out.insert(id) {
                if let Some(p) = peers.get(&id) {
                    todo.extend(p.advert.sources.iter().copied());
                }
            }
        }
        out
    }

    /// Called by the UI poller ~10 times a second: applies volumes, keeps the
    /// beacon current, reconnects, and returns what to draw.
    pub fn tick(&self) -> Snapshot {
        let names = audio::device_names();
        let mix = self.mix.lock().unwrap().clone();
        let peers = self.disco.peers();
        let sources = self.my_sources();

        // Host side: one row per client device, merging its system + mic streams.
        let mut error = self.disco.error.lock().unwrap().clone().map(|e| format!("Can't look for other devices: {e}"));
        let mut stats = Vec::new();
        let hosting = {
            let host = self.host.lock().unwrap();
            host.as_ref().map(|h| {
                if let Some(e) = h.error.lock().unwrap().clone() {
                    error = Some(e);
                }
                let streams = h.shared.streams.lock().unwrap().clone();
                let mut rows: BTreeMap<String, (ClientRow, f32, u32, u32, f32)> = BTreeMap::new();
                for s in &streams {
                    let (key, name, os, source) = match &s.who {
                        Some((w, src)) => (id_str(w.device), w.name.clone(), os_name(w.os), Some(*src)),
                        None => (s.addr.ip().to_string(), s.addr.ip().to_string(), "windows", None),
                    };
                    let m = mix.get(&key).copied().unwrap_or(Mix { volume: 1.0, muted: false });
                    // Perceptual fader: square law.
                    s.stats.set_gain(if m.muted { 0.0 } else { m.volume * m.volume });
                    let e = rows.entry(key.clone()).or_insert_with(|| {
                        (
                            ClientRow { id: key, name, os, sends: Sends::default(), mark: "live", latency_ms: 0.0, recovered: 0, level: 0.0, volume: m.volume, muted: m.muted },
                            0.0,
                            0,
                            0,
                            0.0,
                        )
                    });
                    match source {
                        Some(Source::Mic) => e.0.sends.mic = true,
                        _ => e.0.sends.system = true,
                    }
                    e.0.latency_ms = e.0.latency_ms.max(s.stats.latency_ms());
                    e.0.recovered += s.stats.recovered.load(Relaxed);
                    e.0.level = e.0.level.max(meter(s.peak));
                    e.1 = e.1.max(s.stats.target_ms());
                    e.2 += s.stats.concealed.load(Relaxed);
                    e.3 += s.stats.underruns.load(Relaxed);
                }
                let clients = rows
                    .into_values()
                    .map(|(mut row, target, concealed, dropouts, _)| {
                        row.mark = if row.muted { "muted" } else if row.latency_ms > DEGRADED_MS { "degraded" } else { "live" };
                        stats.push(Stats { name: row.name.clone(), buffer_ms: row.latency_ms, target_ms: target, recovered: row.recovered, concealed, dropouts });
                        row
                    })
                    .collect::<Vec<_>>();
                Hosting { output: names.default_output.clone().unwrap_or_else(|| "default output".into()), clients }
            })
        };

        // Keep our beacon current.
        {
            let mut me = self.disco.me.lock().unwrap();
            me.hosting = hosting.is_some();
            me.clients = hosting.as_ref().map_or(0, |h| h.clients.len() as u32);
            me.sources = sources.iter().copied().collect();
        }

        let host_info = |p: &crate::discovery::Peer| HostInfo {
            id: id_str(p.advert.id),
            name: p.advert.name.clone(),
            os: if p.advert.macos { "macos" } else { "windows" },
            clients: p.advert.clients,
            blocked: sources.contains(&p.advert.id).then(|| "Already sends its audio to this device.".to_string()),
        };
        let hosts: Vec<HostInfo> = peers.iter().filter(|p| p.advert.hosting).map(host_info).collect();

        // Reconnect to the last host as soon as it shows up.
        let (reconnect, last) = {
            let p = self.prefs.lock().unwrap();
            (p.reconnect, p.last_host)
        };
        if reconnect && self.link.lock().unwrap().is_none() {
            if let Some(h) = last.filter(|h| hosts.iter().any(|x| x.id == id_str(*h) && x.blocked.is_none())) {
                let _ = self.connect(h);
            }
        }

        let link = self.link.lock().unwrap().as_ref().map(|l| {
            let runs = [l.system.as_ref(), l.mic.as_ref()];
            let alive = runs.iter().flatten().any(|r| {
                let last = r.shared.last_status_ms.load(Relaxed);
                last > 0 && (r.started.elapsed().as_millis() as u32).saturating_sub(last) < LINK_TIMEOUT_MS
            });
            let latency = runs.iter().flatten().map(|r| r.shared.buffer_ms.get()).fold(0.0, f32::max);
            let fresh = peers.iter().find(|p| p.advert.id == l.host);
            Link {
                host: HostInfo {
                    id: id_str(l.host),
                    name: l.name.clone(),
                    os: if l.macos { "macos" } else { "windows" },
                    clients: fresh.map_or(l.clients, |p| p.advert.clients),
                    blocked: None,
                },
                status: if alive { "connected" } else { "reconnecting" },
                latency_ms: latency,
                send: Sends { system: l.system.is_some(), mic: l.mic.is_some() },
                hear: Sends::default(),
            }
        });

        let output = names.default_output.clone().unwrap_or_default();
        Snapshot {
            me: Me { name: self.me.name.clone(), os: os_name(self.me.os) },
            gate: None,
            searching: peers.is_empty() && self.started.elapsed() < Duration::from_secs(3),
            hosting,
            link,
            hosts,
            quality: "lossless",
            output_lost: false,
            stats,
            devices: Devices {
                outputs: names.outputs,
                mics: names.inputs,
                output: output.clone(),
                mic: names.default_input.unwrap_or_default(),
                system_source: output,
            },
            options: Options { reconnect, start_at_login: false, quiet_wifi: false },
            error,
        }
    }

    pub fn shutdown(&self) {
        self.disconnect();
        self.stop_hosting();
        self.disco.stop.store(true, Relaxed);
    }
}
