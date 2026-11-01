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
use aon_core::protocol::{self, DEFAULT_PORT, Os, Quality, Source};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Above this the row is marked unstable.
const DEGRADED_MS: f32 = 60.0;
/// No Status from the host for this long means the link is down.
const LINK_TIMEOUT_MS: u32 = 3_000;
/// A client re-sends its Want this often; the host drops it after WANT_EXPIRE.
const WANT_EVERY: Duration = Duration::from_secs(1);
const WANT_EXPIRE: Duration = Duration::from_secs(3);

struct HostRun {
    shared: Arc<HubShared>,
    thread: JoinHandle<()>,
    error: Arc<Mutex<Option<String>>>,
    /// Output device choice the hub reads on each (re)open.
    output: Arc<Mutex<Option<String>>>,
    /// Host only: our audio streamed back to clients that asked for it.
    returns: HashMap<(IpAddr, u64, u16, bool), SendRun>,
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
    /// What we asked to hear from the host, and the local hub playing it.
    hear: Sends,
    back: Option<HostRun>,
}

#[derive(Clone, Copy)]
struct Mix {
    volume: f32,
    muted: bool,
}

pub struct Prefs {
    pub reconnect: bool,
    pub last_host: Option<u64>,
    /// This device was hosting when the app last ran; only Stop clears it.
    pub was_hosting: bool,
    /// Chosen devices by name; `None` follows the system default.
    pub output: Option<String>,
    pub system_source: Option<String>,
    pub mic: Option<String>,
    /// What this device sends: Lossless, CD, Balanced or Saver.
    pub quality: Quality,
    /// Shown in the UI; the desktop shell applies them.
    pub start_at_login: bool,
    pub quiet_wifi: bool,
    /// Silence this computer's speakers while it sends its system audio.
    pub mute_speakers: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum DeviceRole {
    Output,
    SystemSource,
    Mic,
}

pub struct Engine {
    me: Identity,
    disco: Arc<Discovery>,
    host: Mutex<Option<HostRun>>,
    link: Mutex<Option<LinkRun>>,
    mix: Mutex<HashMap<String, Mix>>,
    prefs: Mutex<Prefs>,
    want_mic: Mutex<bool>,
    /// Sends Want packets to the host (also a keepalive) and reads its acks.
    ctrl: UdpSocket,
    last_want: Mutex<Instant>,
    last_ack: Mutex<Option<Instant>>,
}

// ---- snapshot: mirrors `AppState` in app/src/lib/state.svelte.ts ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    me: Me,
    gate: Option<&'static str>,
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
    /// What comes back from the host: meter level and buffer depth.
    hear_level: f32,
    /// Level of what this device is sending, for the client's meter.
    send_level: f32,
    hear_latency_ms: f32,
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
    /// Chosen device names; empty means "follow the system default".
    output: String,
    mic: String,
    system_source: String,
    default_output: String,
    default_mic: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    reconnect: bool,
    start_at_login: bool,
    quiet_wifi: bool,
    mute_speakers: bool,
}

impl Snapshot {
    /// One line for the CLI: role, peers and stream health.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(h) = &self.hosting {
            let clients: Vec<String> = h
                .clients
                .iter()
                .map(|c| format!("{} {:.1} ms lvl {:.2} rec {}", c.name, c.latency_ms, c.level, c.recovered))
                .collect();
            parts.push(format!("hosting on {} [{}]", h.output, clients.join("; ")));
        }
        if let Some(l) = &self.link {
            parts.push(format!(
                "{} {} {:.1} ms (send sys {} mic {}, hear sys {} mic {}: lvl {:.2}, {:.1} ms)",
                l.status, l.host.name, l.latency_ms, l.send.system, l.send.mic, l.hear.system, l.hear.mic, l.hear_level, l.hear_latency_ms
            ));
        }
        if self.hosting.is_none() && self.link.is_none() {
            let hosts: Vec<&str> = self.hosts.iter().map(|h| h.name.as_str()).collect();
            parts.push(format!("idle, hosts seen: [{}]", hosts.join(", ")));
        }
        if let Some(e) = &self.error {
            parts.push(format!("error: {e}"));
        }
        parts.join(" | ")
    }

    /// Ids and names of the hosts currently visible.
    pub fn host_ids(&self) -> Vec<(String, String, bool)> {
        self.hosts.iter().map(|h| (h.id.clone(), h.name.clone(), h.blocked.is_none())).collect()
    }
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
        // Speakers a crashed run left muted come back on.
        #[cfg(windows)]
        crate::speakers::recover();
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
            ctrl: {
                let s = UdpSocket::bind(("0.0.0.0", 0)).expect("bind control socket");
                s.set_nonblocking(true).expect("nonblocking control socket");
                s
            },
            last_want: Mutex::new(Instant::now()),
            last_ack: Mutex::new(None),
        })
    }

    pub fn last_host(&self) -> Option<u64> {
        self.prefs.lock().unwrap().last_host
    }

    pub fn set_reconnect(&self, on: bool) {
        self.prefs.lock().unwrap().reconnect = on;
    }

    pub fn prefs(&self) -> std::sync::MutexGuard<'_, Prefs> {
        self.prefs.lock().unwrap()
    }

    /// Pick a device by name (`None` = system default) and apply it live.
    pub fn set_device(&self, role: DeviceRole, name: Option<String>) {
        {
            let mut p = self.prefs.lock().unwrap();
            match role {
                DeviceRole::Output => p.output = name,
                DeviceRole::SystemSource => p.system_source = name,
                DeviceRole::Mic => p.mic = name,
            }
        }
        match role {
            DeviceRole::Output => {
                if let Some(h) = self.host.lock().unwrap().as_ref() {
                    h.output.lock().unwrap().clone_from(&self.prefs.lock().unwrap().output);
                    h.shared.reopen.store(true, Relaxed);
                }
            }
            DeviceRole::SystemSource | DeviceRole::Mic => {
                // Restart the affected sender on the new device.
                let source = if matches!(role, DeviceRole::Mic) { Source::Mic } else { Source::System };
                let on = self.link.lock().unwrap().as_ref().is_some_and(|l| match source {
                    Source::System => l.system.is_some(),
                    Source::Mic => l.mic.is_some(),
                });
                if on {
                    self.set_send(source, false);
                    self.set_send(source, true);
                }
            }
        }
    }

    /// Mute or unmute the speakers while sending; a running system stream restarts.
    pub fn set_mute_speakers(&self, on: bool) {
        self.prefs.lock().unwrap().mute_speakers = on;
        if self.link.lock().unwrap().as_ref().is_some_and(|l| l.system.is_some()) {
            self.set_send(Source::System, false);
            self.set_send(Source::System, true);
        }
    }

    /// Change what this device sends; running streams restart on the new codec.
    pub fn set_quality(&self, quality: Quality) {
        self.prefs.lock().unwrap().quality = quality;
        let (system, mic) = self.link.lock().unwrap().as_ref().map_or((false, false), |l| (l.system.is_some(), l.mic.is_some()));
        for (source, on) in [(Source::System, system), (Source::Mic, mic)] {
            if on {
                self.set_send(source, false);
                self.set_send(source, true);
            }
        }
        // Streams back to clients are recreated on the next tick.
        if let Some(h) = self.host.lock().unwrap().as_mut() {
            for (_, r) in h.returns.drain() {
                r.shared.stop.store(true, Relaxed);
            }
        }
    }

    pub fn was_hosting(&self) -> bool {
        self.prefs.lock().unwrap().was_hosting
    }

    pub fn become_host(&self) -> Result<()> {
        self.prefs.lock().unwrap().was_hosting = true;
        let mut host = self.host.lock().unwrap();
        if host.is_none() {
            *host = Some(self.spawn_hub(DEFAULT_PORT)?);
        }
        Ok(())
    }

    /// Start a hub on `port` (0 = any) playing on the chosen output.
    fn spawn_hub(&self, port: u16) -> Result<HostRun> {
        let shared = Arc::new(HubShared::default());
        let error = Arc::new(Mutex::new(None));
        let output = Arc::new(Mutex::new(self.prefs.lock().unwrap().output.clone()));
        let (s, e, o) = (shared.clone(), error.clone(), output.clone());
        let thread = std::thread::Builder::new().name("aonin-hub".into()).spawn(move || {
            let run = || -> Result<()> {
                // A chosen device is reread on every (re)open so a new pick
                // takes effect; with none chosen, follow the system default.
                let open = || audio::start_output(o.lock().unwrap().as_deref());
                let follow = o.lock().unwrap().is_none();
                net::run_hub(port, open, follow, &s, false)
            };
            if let Err(err) = run() {
                *e.lock().unwrap() = Some(format!("Couldn't start hosting: {err}"));
            }
        })?;
        Ok(HostRun { shared, thread, error, output, returns: HashMap::new() })
    }

    fn stop_hub(h: HostRun) {
        for (_, r) in h.returns {
            Self::stop_send(Some(r));
        }
        h.shared.stop.store(true, Relaxed);
        let _ = h.thread.join();
    }

    /// The user's Stop: also forget that this device hosts.
    pub fn stop_hosting(&self) {
        self.prefs.lock().unwrap().was_hosting = false;
        self.close_hub();
    }

    /// Stop the hub but keep remembering the role (quitting the app).
    fn close_hub(&self) {
        if let Some(h) = self.host.lock().unwrap().take() {
            Self::stop_hub(h);
        }
    }

    fn start_send(&self, addr: SocketAddr, source: Source) -> SendRun {
        let shared = Arc::new(SendShared::default());
        let s = shared.clone();
        let who = self.me.clone();
        let (device, quality, mute) = {
            let p = self.prefs.lock().unwrap();
            let device = match source {
                Source::System => p.system_source.clone(),
                Source::Mic => p.mic.clone(),
            };
            (device, p.quality, p.mute_speakers)
        };
        let thread = std::thread::Builder::new()
            .name(format!("aonin-send-{source:?}"))
            .spawn(move || {
                let kind = match source {
                    Source::System => audio::Source::System,
                    Source::Mic => audio::Source::Mic,
                };
                let run = || -> Result<()> {
                    let (_stream, cap, rate) = audio::start_capture(kind, device.as_deref(), mute)?;
                    net::run_sender(SendConfig { hub: addr, who, source, quality, drop_percent: 0, verbose: false }, cap, rate, &s)
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
        *self.last_ack.lock().unwrap() = None;
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
            hear: Sends::default(),
            back: None,
        });
        self.prefs.lock().unwrap().last_host = Some(host);
        Ok(())
    }

    pub fn disconnect(&self) {
        if let Some(l) = self.link.lock().unwrap().take() {
            if l.hear.system || l.hear.mic {
                self.send_want(l.addr, 0, Sends::default());
            }
            Self::stop_send(l.system);
            Self::stop_send(l.mic);
            if let Some(b) = l.back {
                Self::stop_hub(b);
            }
        }
    }

    fn send_want(&self, host: SocketAddr, port: u16, hear: Sends) {
        let mut buf = Vec::new();
        protocol::encode_want(&mut buf, self.me.device, port, hear.system, hear.mic);
        let _ = self.ctrl.send_to(&buf, host);
    }

    /// Ask the host to stream its own audio and/or mic back to us, played on
    /// our output through a local hub.
    pub fn set_hear(&self, source: Source, on: bool) -> Result<()> {
        let mut link = self.link.lock().unwrap();
        let Some(l) = link.as_mut() else { return Ok(()) };
        match source {
            Source::System => l.hear.system = on,
            Source::Mic => l.hear.mic = on,
        }
        let any = l.hear.system || l.hear.mic;
        if any && l.back.is_none() {
            l.back = Some(self.spawn_hub(0)?);
        }
        let port = l.back.as_ref().map_or(0, |b| b.shared.port.load(Relaxed));
        self.send_want(l.addr, port, l.hear);
        if !any {
            if let Some(b) = l.back.take() {
                Self::stop_hub(b);
            }
        }
        Ok(())
    }

    /// Host side: start or stop the streams back to clients that asked.
    fn serve_returns(&self, h: &mut HostRun) {
        let wants = {
            let mut w = h.shared.wants.lock().unwrap();
            w.retain(|_, want| want.at.elapsed() < WANT_EXPIRE);
            w.clone()
        };
        let mut needed = HashSet::new();
        for ((ip, device), w) in &wants {
            for (on, mic) in [(w.system, false), (w.mic, true)] {
                if on && w.port != 0 {
                    needed.insert((*ip, *device, w.port, mic));
                }
            }
        }
        h.returns.retain(|key, run| {
            let keep = needed.contains(key);
            if !keep {
                run.shared.stop.store(true, Relaxed);
            }
            keep
        });
        for key in needed {
            if !h.returns.contains_key(&key) {
                let (ip, _, port, mic) = key;
                let source = if mic { Source::Mic } else { Source::System };
                h.returns.insert(key, self.start_send(SocketAddr::new(ip, port), source));
            }
        }
    }

    /// The user's explicit disconnect also stops auto-reconnect to that host.
    /// Forget first: disconnecting takes a moment (senders wind down), and a
    /// tick in between would otherwise reconnect straight away.
    pub fn forget_host(&self) {
        self.prefs.lock().unwrap().last_host = None;
        self.disconnect();
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
        let mut output_lost = false;
        if let Some(h) = self.host.lock().unwrap().as_mut() {
            self.serve_returns(h);
        }
        let hosting = {
            let host = self.host.lock().unwrap();
            host.as_ref().map(|h| {
                if let Some(e) = h.error.lock().unwrap().clone() {
                    error = Some(e);
                }
                output_lost = h.shared.output_lost.load(Relaxed);
                let playing = h.shared.output.lock().unwrap().clone();
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
                Hosting { output: if playing.is_empty() { "no output".into() } else { playing }, clients }
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
            blocked: sources.contains(&p.advert.id).then(|| "Joining would play your own audio back to you.".to_string()),
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

        let mut buf = [0u8; 256];
        while let Ok((n, _)) = self.ctrl.recv_from(&mut buf) {
            if let Some(protocol::Packet::Status { .. }) = protocol::decode(&buf[..n]) {
                *self.last_ack.lock().unwrap() = Some(Instant::now());
            }
        }
        if let Some(l) = self.link.lock().unwrap().as_ref() {
            let mut last = self.last_want.lock().unwrap();
            if last.elapsed() >= WANT_EVERY {
                let port = l.back.as_ref().map_or(0, |b| b.shared.port.load(Relaxed));
                self.send_want(l.addr, port, l.hear);
                *last = Instant::now();
            }
        }
        let link = self.link.lock().unwrap().as_ref().map(|l| {
            let runs = [l.system.as_ref(), l.mic.as_ref()];
            let acked = self.last_ack.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(LINK_TIMEOUT_MS as u64));
            let alive = acked
                || runs.iter().flatten().any(|r| {
                    let last = r.shared.last_status_ms.load(Relaxed);
                    last > 0 && (r.started.elapsed().as_millis() as u32).saturating_sub(last) < LINK_TIMEOUT_MS
                });
            let latency = runs.iter().flatten().map(|r| r.shared.buffer_ms.get()).fold(0.0, f32::max);
            let fresh = peers.iter().find(|p| p.advert.id == l.host);
            let send_level = runs.iter().flatten().map(|r| meter(r.shared.peak.take())).fold(0.0, f32::max);
            let (hear_level, hear_latency_ms) = l.back.as_ref().map_or((0.0, 0.0), |b| {
                let streams = b.shared.streams.lock().unwrap();
                streams.iter().fold((0.0f32, 0.0f32), |(lvl, lat), st| (lvl.max(meter(st.peak)), lat.max(st.stats.latency_ms())))
            });
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
                hear: l.hear,
                hear_level,
                send_level,
                hear_latency_ms,
            }
        });

        let prefs = self.prefs.lock().unwrap();
        let devices = Devices {
            output: prefs.output.clone().unwrap_or_default(),
            system_source: prefs.system_source.clone().unwrap_or_default(),
            mic: prefs.mic.clone().unwrap_or_default(),
            default_output: names.default_output.clone().unwrap_or_default(),
            default_mic: names.default_input.clone().unwrap_or_default(),
            outputs: names.outputs,
            mics: names.inputs,
        };
        // Copy what the snapshot needs and release the lock here: a guard
        // taken inside the struct literal below would live to the end of the
        // statement, and a second one would deadlock.
        let (quality, start_at_login, quiet_wifi, mute_speakers) = (prefs.quality.name(), prefs.start_at_login, prefs.quiet_wifi, prefs.mute_speakers);
        drop(prefs);
        Snapshot {
            me: Me { name: self.me.name.clone(), os: os_name(self.me.os) },
            gate: None,
            hosting,
            link,
            hosts,
            quality,
            output_lost,
            stats,
            devices,
            options: Options { reconnect, start_at_login, quiet_wifi, mute_speakers },
            error,
        }
    }

    /// Quit: stop everything but keep the remembered host and role.
    pub fn shutdown(&self) {
        self.disconnect();
        self.close_hub();
        self.disco.stop.store(true, Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Become host is remembered, quitting keeps it, only Stop forgets it.
    #[test]
    fn host_role_is_remembered_until_stop() {
        let prefs = Prefs {
            reconnect: false,
            last_host: None,
            was_hosting: false,
            output: None,
            system_source: None,
            mic: None,
            quality: Quality::Lossless,
            start_at_login: false,
            quiet_wifi: false,
            mute_speakers: false,
        };
        let engine = Engine::new(2, "test".into(), prefs);
        engine.become_host().unwrap();
        assert!(engine.was_hosting());
        engine.shutdown();
        assert!(engine.was_hosting(), "quitting keeps the role");
        engine.become_host().unwrap();
        engine.stop_hosting();
        assert!(!engine.was_hosting(), "Stop forgets it");
    }

    /// A lock held across the snapshot once froze the UI: tick must return.
    #[test]
    fn tick_returns_and_never_deadlocks() {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let prefs = Prefs {
                reconnect: true,
                last_host: None,
                was_hosting: false,
                output: None,
                system_source: None,
                mic: None,
                quality: Quality::Cd,
                start_at_login: true,
                quiet_wifi: false,
                mute_speakers: false,
            };
            let engine = Engine::new(1, "test".into(), prefs);
            let snaps: Vec<String> = (0..3).map(|_| engine.tick().quality.to_string()).collect();
            engine.shutdown();
            let _ = tx.send(snaps);
        });
        let snaps = rx.recv_timeout(Duration::from_secs(20)).expect("tick() hung");
        assert!(snaps.iter().all(|q| q == "cd"));
    }
}
