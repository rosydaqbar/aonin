//! The app-facing engine: host/client roles on top of the sender, hub and
//! discovery loops, plus the snapshot the UI renders.
//!
//! Audio streams (cpal) are not `Send` on every platform, so each role runs
//! in its own thread that opens its device and keeps the stream alive until
//! told to stop.

use crate::audio;
use crate::discovery::{Advert, Discovery};
use crate::net::{self, HubShared, Identity, PrivateHub, SendConfig, SendShared};
use crate::private::{self, Cipher, Pairing, Upgrade};
use crate::pro::{Edition, Feature, Pro};
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
/// Handover: how long the new listener has to start, and how long the old
/// one keeps announcing where everyone went (for senders that missed it).
const HANDOVER_START: Duration = Duration::from_secs(8);
const HANDOVER_ANNOUNCE: Duration = Duration::from_secs(10);
/// Joining with a code: PairStart is resent this often until the listener
/// answers, and the attempt gives up after JOIN_TIMEOUT.
const JOIN_RESEND: Duration = Duration::from_millis(300);
const JOIN_TIMEOUT: Duration = Duration::from_secs(3);
/// How often a lost private link tries to pair again with the saved code.
const REJOIN_EVERY: Duration = Duration::from_secs(4);
/// How long an audio sender asks a listener that turned private for the
/// code before it gives up and disconnects.
const UPGRADE_WAIT: Duration = Duration::from_secs(6);

/// Looking for the private listener that has a code.
struct Join {
    code: String,
    /// Each private listener tried, by its hub address.
    tries: HashMap<SocketAddr, (u64, Pairing)>,
    started: Instant,
    last_send: Option<Instant>,
    /// Reconnecting with a saved code: no error shown if it fails.
    quiet: bool,
    /// Reconnecting: only this listener is tried.
    only: Option<u64>,
}

/// This device is handing its listener role to `to`.
struct Handover {
    to: u64,
    name: String,
    since: Instant,
    /// When `to` started listening and this device stopped.
    moved: Option<Instant>,
}

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
    /// Private session key shared with this listener.
    cipher: Option<Arc<Cipher>>,
    since: Instant,
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
    /// What this device sends: Highest, CD, Balanced or Saver.
    pub quality: Quality,
    /// Shown in the UI; the desktop shell applies them.
    pub start_at_login: bool,
    pub quiet_wifi: bool,
    /// Silence this computer's speakers while it sends its system audio.
    pub mute_speakers: bool,
    /// The name others see while this device listens; `None` uses the
    /// device name.
    pub listen_name: Option<String>,
    /// Set while listening privately: the code audio senders type.
    pub private_code: Option<String>,
    /// The code of `last_host` when it is private, to rejoin without asking.
    pub last_code: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            reconnect: true,
            last_host: None,
            was_hosting: false,
            output: None,
            system_source: None,
            mic: None,
            quality: Quality::default(),
            start_at_login: false,
            quiet_wifi: false,
            mute_speakers: false,
            listen_name: None,
            private_code: None,
            last_code: None,
        }
    }
}

/// What the shell persists between launches; compare to know when to save.
#[derive(Clone, PartialEq, Debug)]
pub struct Saved {
    pub last_host: Option<u64>,
    pub was_hosting: bool,
    pub last_code: Option<String>,
    pub listen_name: Option<String>,
    pub private_code: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum DeviceRole {
    Output,
    SystemSource,
    Mic,
}

pub struct Engine {
    me: Identity,
    /// What this edition unlocks (see `pro`).
    pro: Arc<dyn Pro>,
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
    /// macOS refused to let us reach the host (Local Network privacy).
    local_net_blocked: std::sync::atomic::AtomicBool,
    perms: crate::permissions::Checker,
    handover: Mutex<Option<Handover>>,
    /// Why the last handover didn't happen; shown until the next one.
    handover_error: Mutex<Option<String>>,
    join: Mutex<Option<Join>>,
    /// Sending to a listener that just turned private: asking it for the
    /// code (listener id, request, started, last sent).
    upgrade: Mutex<Option<(u64, Upgrade, Instant, Option<Instant>)>>,
    /// Why the last join with a code failed; shown until the next try.
    join_error: Mutex<Option<String>>,
    last_rejoin: Mutex<Option<Instant>>,
}

// ---- snapshot: mirrors `AppState` in app/src/lib/state.svelte.ts ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    me: Me,
    /// What the OS is stopping Aonin from doing (see `permissions`).
    issues: Vec<&'static str>,
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
    /// Name of the device this one is handing the listener role to.
    handing_over: Option<String>,
    /// Looking for the listener that has the code the user typed.
    joining: bool,
    join_error: Option<String>,
    edition: Edition,
}

#[derive(Serialize)]
struct Me {
    name: String,
    os: &'static str,
}

#[derive(Serialize)]
struct Hosting {
    /// The name audio senders see.
    name: String,
    /// Set when private: the code audio senders type to join.
    code: Option<String>,
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

    pub fn is_linked(&self) -> bool {
        self.link.is_some()
    }

    pub fn join_failed(&self) -> Option<&str> {
        if self.joining { None } else { self.join_error.as_deref() }
    }

    /// Ids and names of the devices sending to this listener.
    pub fn client_ids(&self) -> Vec<(String, String)> {
        self.hosting.as_ref().map_or(Vec::new(), |h| h.clients.iter().map(|c| (c.id.clone(), c.name.clone())).collect())
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
    pub fn new(device: u64, name: String, prefs: Prefs, pro: Arc<dyn Pro>) -> Arc<Self> {
        // Speakers a crashed run left muted come back on.
        #[cfg(windows)]
        crate::speakers::recover();
        let os = if cfg!(target_os = "macos") { Os::MacOs } else { Os::Windows };
        let disco = Arc::new(Discovery::default());
        *disco.me.lock().unwrap() = Advert { id: device, name: name.clone(), macos: os == Os::MacOs, port: DEFAULT_PORT, ..Default::default() };
        let d = disco.clone();
        std::thread::Builder::new().name("aonin-discovery".into()).spawn(move || d.run()).expect("spawn discovery");
        // macOS shows its Local Network prompt now rather than mid-stream.
        crate::permissions::advertise(DEFAULT_PORT);
        Arc::new(Self {
            me: Identity { device, name, os },
            pro,
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
            local_net_blocked: Default::default(),
            perms: crate::permissions::Checker::start(),
            handover: Mutex::new(None),
            handover_error: Mutex::new(None),
            join: Mutex::new(None),
            upgrade: Mutex::new(None),
            join_error: Mutex::new(None),
            last_rejoin: Mutex::new(None),
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
        {
            let mut host = self.host.lock().unwrap();
            if host.is_none() {
                *host = Some(self.spawn_hub(DEFAULT_PORT)?);
            }
        }
        self.apply_private();
        Ok(())
    }

    /// Start listening under `name` (`None`: the device name), privately when
    /// `code` is set.
    pub fn start_listening(&self, name: Option<String>, code: Option<String>) -> Result<()> {
        let code = code.map(|c| private::normalize_code(&c).ok_or_else(|| anyhow!("A code has 6 letters and numbers."))).transpose()?;
        if code.is_some() {
            self.pro.allow(Feature::MakePrivate)?;
        }
        {
            let mut p = self.prefs.lock().unwrap();
            p.listen_name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
            p.private_code = code;
        }
        self.become_host()
    }

    /// Make the running listener private with `code`, or public with `None`.
    /// Audio senders without the code are dropped when it turns private.
    pub fn set_private(&self, code: Option<String>) -> Result<()> {
        let code = code.map(|c| private::normalize_code(&c).ok_or_else(|| anyhow!("A code has 6 letters and numbers."))).transpose()?;
        // Only a listener can be private; an audio sender has no such choice.
        if self.host.lock().unwrap().is_none() {
            return Err(anyhow!("Only the listener can be private."));
        }
        if code.is_some() {
            self.pro.allow(Feature::MakePrivate)?;
        }
        if code.is_some() && self.handover.lock().unwrap().is_some() {
            return Err(anyhow!("Wait for the handover to finish."));
        }
        // Devices sending here right now come along: they may ask for the
        // code for a little while (see `private::Upgrade`).
        if code.is_some()
            && let Some(h) = self.host.lock().unwrap().as_ref()
        {
            let now = Instant::now();
            let mut g = h.shared.grandfather.lock().unwrap();
            for s in h.shared.streams.lock().unwrap().iter() {
                g.insert(s.addr.ip(), now);
            }
            for (ip, _) in h.shared.wants.lock().unwrap().keys() {
                g.insert(*ip, now);
            }
        }
        self.prefs.lock().unwrap().private_code = code;
        self.apply_private();
        Ok(())
    }

    /// The name audio senders see while this device listens.
    fn listen_name(&self) -> String {
        self.prefs.lock().unwrap().listen_name.clone().unwrap_or_else(|| self.me.name.clone())
    }

    /// Tell the running hub whether it is private, and with which code.
    fn apply_private(&self) {
        let code = self.prefs.lock().unwrap().private_code.clone();
        let session = code.map(|code| PrivateHub { code, name: self.listen_name(), ops: self.pro.private_hub() });
        if let Some(h) = self.host.lock().unwrap().as_ref() {
            *h.shared.private.lock().unwrap() = session;
        }
    }

    /// What the shell saves between launches.
    pub fn saved(&self) -> Saved {
        let p = self.prefs.lock().unwrap();
        Saved {
            last_host: p.last_host,
            was_hosting: p.was_hosting,
            last_code: p.last_code.clone(),
            listen_name: p.listen_name.clone(),
            private_code: p.private_code.clone(),
        }
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
                *e.lock().unwrap() = Some(format!("Couldn't become the listener: {err}"));
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

    fn start_send(&self, addr: SocketAddr, source: Source, cipher: Option<Arc<Cipher>>) -> SendRun {
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
                    net::run_sender(SendConfig { hub: addr, who, source, quality, drop_percent: 0, verbose: false, cipher }, cap, rate, &s)
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
        self.connect_with(host, None)
    }

    /// Connect to `host`; `private` is the name and key from pairing with a
    /// private listener.
    fn connect_with(&self, host: u64, private: Option<(String, Arc<Cipher>, String)>) -> Result<()> {
        let mut peer = self.disco.peers.lock().unwrap().get(&host).cloned().ok_or_else(|| anyhow!("That listener is no longer on the network."))?;
        // Rejoining the same listener (it turned private, or restarted)
        // keeps what we asked to hear from it.
        let hear = self.link.lock().unwrap().as_ref().filter(|l| l.host == host).map(|l| l.hear).unwrap_or_default();
        let (cipher, code) = match private {
            Some((name, cipher, code)) => {
                peer.advert.name = name;
                (Some(cipher), Some(code))
            }
            None if peer.advert.private => return Err(anyhow!("That listener is private. Join it with its code.")),
            None => (None, None),
        };
        if !peer.advert.hosting {
            return Err(anyhow!("{} stopped listening.", peer.advert.name));
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
            system: Some(self.start_send(addr, Source::System, cipher.clone())),
            mic: mic.then(|| self.start_send(addr, Source::Mic, cipher.clone())),
            hear: Sends::default(),
            back: None,
            cipher,
            since: Instant::now(),
        });
        {
            let mut p = self.prefs.lock().unwrap();
            p.last_host = Some(host);
            p.last_code = code;
        }
        if hear.system {
            self.set_hear(Source::System, true)?;
        }
        if hear.mic {
            self.set_hear(Source::Mic, true)?;
        }
        Ok(())
    }

    pub fn disconnect(&self) {
        if let Some(l) = self.link.lock().unwrap().take() {
            if l.hear.system || l.hear.mic {
                self.send_want(l.addr, 0, Sends::default(), l.cipher.as_deref());
            }
            Self::stop_send(l.system);
            Self::stop_send(l.mic);
            if let Some(b) = l.back {
                Self::stop_hub(b);
            }
        }
    }

    fn send_want(&self, host: SocketAddr, port: u16, hear: Sends, cipher: Option<&Cipher>) {
        let mut buf = Vec::new();
        protocol::encode_want(&mut buf, self.me.device, port, hear.system, hear.mic);
        if let Some(c) = cipher {
            let plain = std::mem::take(&mut buf);
            c.seal(&plain, &mut buf);
        }
        // Without Local Network access macOS drops every packet to the LAN,
        // but reports it ("no route to host", EHOSTUNREACH) only on the first
        // few sends; later ones look fine. So the flag stays set until the
        // host actually answers (see `tick`).
        if let Err(e) = self.ctrl.send_to(&buf, host)
            && cfg!(target_os = "macos")
            && e.raw_os_error() == Some(65)
            && !self.local_net_blocked.swap(true, Relaxed)
        {
            eprintln!("macOS blocks Aonin from {host}: Local Network access is off");
        }
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
            let back = self.spawn_hub(0)?;
            // The listener's audio comes back sealed with the same key.
            if let Some(c) = &l.cipher {
                back.shared.keys.lock().unwrap().insert(l.addr.ip(), c.clone());
            }
            l.back = Some(back);
        }
        let port = l.back.as_ref().map_or(0, |b| b.shared.port.load(Relaxed));
        self.send_want(l.addr, port, l.hear, l.cipher.as_deref());
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
                let cipher = h.shared.keys.lock().unwrap().get(&ip).cloned();
                h.returns.insert(key, self.start_send(SocketAddr::new(ip, port), source, cipher));
            }
        }
    }

    /// Listener: hand the role to `client`, one of the devices sending here.
    /// It starts listening, every sender follows it, and this device then
    /// sends to it too. All of it travels in the beacon (see `follow_handover`).
    pub fn hand_over(&self, client: u64) -> Result<()> {
        self.pro.allow(Feature::HandOver)?;
        let peer = self.disco.peers.lock().unwrap().get(&client).cloned().ok_or_else(|| anyhow!("That device is no longer on the network."))?;
        let sending_here = self
            .host
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|h| h.shared.streams.lock().unwrap().iter().any(|s| s.who.as_ref().is_some_and(|(w, _)| w.device == client)));
        if !sending_here {
            return Err(anyhow!("{} isn't sending to this device.", peer.advert.name));
        }
        if self.prefs.lock().unwrap().private_code.is_some() {
            return Err(anyhow!("A private listener can't hand over yet. Make it public first."));
        }
        *self.handover_error.lock().unwrap() = None;
        *self.handover.lock().unwrap() = Some(Handover { to: client, name: peer.advert.name, since: Instant::now(), moved: None });
        self.disco.me.lock().unwrap().handover = Some(client);
        Ok(())
    }

    /// Run every tick. Three parts, one per device in a handover:
    /// - the old listener waits for the new one, then stops and sends to it;
    /// - the new listener, told by the beacon of the listener it sends to,
    ///   stops sending and starts listening;
    /// - every other sender moves its running streams to the new listener.
    fn follow_handover(&self, peers: &[crate::discovery::Peer]) {
        let peer = |id: u64| peers.iter().find(|p| p.advert.id == id);

        // Old listener.
        let mine = self.handover.lock().unwrap().as_ref().map(|h| (h.to, h.name.clone(), h.since, h.moved));
        if let Some((to, name, since, moved)) = mine {
            let listening = peer(to).is_some_and(|p| p.advert.hosting);
            match moved {
                None if listening => {
                    self.prefs.lock().unwrap().was_hosting = false;
                    self.close_hub();
                    if let Some(h) = self.handover.lock().unwrap().as_mut() {
                        h.moved = Some(Instant::now());
                    }
                }
                None if since.elapsed() > HANDOVER_START => {
                    *self.handover.lock().unwrap() = None;
                    self.disco.me.lock().unwrap().handover = None;
                    *self.handover_error.lock().unwrap() = Some(format!("{name} didn't take over. This device is still the listener."));
                }
                Some(at) => {
                    let linked = self.link.lock().unwrap().as_ref().map(|l| l.host);
                    if linked != Some(to) && listening {
                        if linked.is_some() {
                            self.disconnect();
                        }
                        let _ = self.connect(to);
                    }
                    if at.elapsed() > HANDOVER_ANNOUNCE {
                        *self.handover.lock().unwrap() = None;
                        self.disco.me.lock().unwrap().handover = None;
                    }
                }
                None => {}
            }
            return;
        }

        // A sender: does the listener it sends to hand over?
        let Some(host) = self.link.lock().unwrap().as_ref().map(|l| l.host) else { return };
        let Some(to) = peer(host).and_then(|p| p.advert.handover) else { return };
        if to == self.me.device {
            // We are the new listener. Our audio plays here from now on.
            self.disconnect();
            {
                // The old listener was public, and so is this one.
                let mut p = self.prefs.lock().unwrap();
                p.last_host = None;
                p.private_code = None;
                p.listen_name = None;
            }
            if let Err(e) = self.become_host() {
                eprintln!("handover: {e}");
            }
        } else if peer(to).is_some_and(|p| p.advert.hosting) && !self.my_sources().contains(&to) {
            self.move_link(to, peers);
        }
    }

    /// Point the running streams at another listener without stopping them.
    fn move_link(&self, to: u64, peers: &[crate::discovery::Peer]) {
        let Some(p) = peers.iter().find(|p| p.advert.id == to) else { return };
        let addr = SocketAddr::new(p.ip, p.advert.port);
        {
            let mut link = self.link.lock().unwrap();
            let Some(l) = link.as_mut() else { return };
            let hearing = l.hear.system || l.hear.mic;
            if hearing {
                self.send_want(l.addr, 0, Sends::default(), None);
            }
            for r in [l.system.as_ref(), l.mic.as_ref()].into_iter().flatten() {
                *r.shared.retarget.lock().unwrap() = Some(addr);
            }
            l.host = to;
            l.name = p.advert.name.clone();
            l.macos = p.advert.macos;
            l.clients = p.advert.clients;
            l.addr = addr;
            if hearing {
                let port = l.back.as_ref().map_or(0, |b| b.shared.port.load(Relaxed));
                self.send_want(addr, port, l.hear, None);
            }
        }
        self.prefs.lock().unwrap().last_host = Some(to);
    }

    /// The user's explicit disconnect also stops auto-reconnect to that host.
    /// Forget first: disconnecting takes a moment (senders wind down), and a
    /// tick in between would otherwise reconnect straight away.
    pub fn forget_host(&self) {
        {
            let mut p = self.prefs.lock().unwrap();
            p.last_host = None;
            p.last_code = None;
        }
        self.disconnect();
    }

    /// Join the private listener that shows `code`. The answer arrives over
    /// the next ticks: a link, or `join_error`.
    pub fn join_code(&self, code: &str) -> Result<()> {
        let code = private::normalize_code(code).ok_or_else(|| anyhow!("A code has 6 letters and numbers."))?;
        *self.join_error.lock().unwrap() = None;
        *self.join.lock().unwrap() = Some(Join { code, tries: HashMap::new(), started: Instant::now(), last_send: None, quiet: false, only: None });
        Ok(())
    }

    pub fn cancel_join(&self) {
        *self.join.lock().unwrap() = None;
        *self.join_error.lock().unwrap() = None;
    }

    /// Pair again with the saved code (the listener restarted, or Aonin did).
    fn rejoin(&self, host: u64, code: String) {
        let mut last = self.last_rejoin.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < REJOIN_EVERY) || self.join.lock().unwrap().is_some() {
            return;
        }
        *last = Some(Instant::now());
        *self.join.lock().unwrap() = Some(Join { code, tries: HashMap::new(), started: Instant::now(), last_send: None, quiet: true, only: Some(host) });
    }

    /// Send PairStart to every private listener in view until one answers.
    fn drive_join(&self, peers: &[crate::discovery::Peer]) {
        let sources = self.my_sources();
        let mut guard = self.join.lock().unwrap();
        let Some(j) = guard.as_mut() else { return };
        let all_wrong = !j.tries.is_empty() && j.tries.values().all(|(_, p)| p.answered());
        if j.started.elapsed() > JOIN_TIMEOUT || all_wrong {
            let quiet = j.quiet;
            *guard = None;
            if !quiet {
                let msg = if all_wrong { "That code didn't work. Check it and try again." } else { "No listener has that code. Check it and try again." };
                *self.join_error.lock().unwrap() = Some(msg.into());
            }
            return;
        }
        for p in peers {
            let a = &p.advert;
            if a.hosting && a.private && j.only.is_none_or(|o| o == a.id) && !sources.contains(&a.id) {
                let code = &j.code;
                j.tries.entry(SocketAddr::new(p.ip, a.port)).or_insert_with(|| (a.id, Pairing::new(code)));
            }
        }
        if j.last_send.is_none_or(|t| t.elapsed() >= JOIN_RESEND) {
            for (addr, (_, p)) in &j.tries {
                if !p.answered() {
                    let _ = self.ctrl.send_to(&p.start, addr);
                }
            }
            j.last_send = Some(Instant::now());
        }
    }

    /// The listener we send to turned private: ask it for the code, which it
    /// gives to devices that were connected at that moment.
    fn drive_upgrade(&self, peers: &[crate::discovery::Peer]) {
        if self.upgrade_step(peers) {
            // It never gave us the code: we joined after it turned private.
            self.forget_host();
        }
    }

    /// One step of `drive_upgrade`; true when it is time to give up.
    fn upgrade_step(&self, peers: &[crate::discovery::Peer]) -> bool {
        let link = self.link.lock().unwrap().as_ref().map(|l| (l.host, l.addr, l.cipher.is_some()));
        let need = link.filter(|(host, _, sealed)| !sealed && peers.iter().any(|p| p.advert.id == *host && p.advert.private));
        let mut up = self.upgrade.lock().unwrap();
        let Some((host, addr, _)) = need else {
            *up = None;
            return false;
        };
        if up.as_ref().is_none_or(|u| u.0 != host) {
            *up = Some((host, Upgrade::default(), Instant::now(), None));
        }
        let u = up.as_mut().unwrap();
        // The listener answers at once if it will; a few seconds is plenty.
        if u.2.elapsed() > UPGRADE_WAIT {
            *up = None;
            return true;
        }
        if u.3.is_none_or(|t| t.elapsed() >= JOIN_RESEND) {
            let _ = self.ctrl.send_to(&u.1.request, addr);
            u.3 = Some(Instant::now());
        }
        false
    }

    /// The listener sent its code: join with it right away.
    fn on_upgrade_reply(&self, data: &[u8]) {
        let got = {
            let mut up = self.upgrade.lock().unwrap();
            let Some(u) = up.as_ref() else { return };
            let Some(code) = u.1.finish(data) else { return };
            let host = u.0;
            *up = None;
            (host, code)
        };
        let (host, code) = got;
        *self.join.lock().unwrap() = Some(Join { code, tries: HashMap::new(), started: Instant::now(), last_send: None, quiet: true, only: Some(host) });
    }

    /// A listener answered our PairStart: connect if the code was right.
    fn on_pair_reply(&self, data: &[u8], from: SocketAddr) {
        let paired = {
            let mut guard = self.join.lock().unwrap();
            let Some(j) = guard.as_mut() else { return };
            let Some((id, p)) = j.tries.get_mut(&from) else { return };
            let id = *id;
            let Some((cipher, name)) = p.finish(data) else { return };
            let code = j.code.clone();
            *guard = None;
            (id, name, cipher, code)
        };
        let (id, name, cipher, code) = paired;
        if let Err(e) = self.connect_with(id, Some((name, Arc::new(cipher), code))) {
            *self.join_error.lock().unwrap() = Some(e.to_string());
        }
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
            *slot = Some(self.start_send(l.addr, source, l.cipher.clone()));
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
        let (listen_name, code) = (self.listen_name(), self.prefs.lock().unwrap().private_code.clone());
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
                Hosting { name: listen_name.clone(), code: code.clone(), output: if playing.is_empty() { "no output".into() } else { playing }, clients }
            })
        };

        // Keep our beacon current.
        {
            let mut me = self.disco.me.lock().unwrap();
            me.hosting = hosting.is_some();
            // A private listener leaves its name out: only devices with the
            // code learn it, and 0.1.x devices skip a beacon without one.
            me.private = me.hosting && code.is_some();
            me.name = if me.private { String::new() } else if me.hosting { listen_name.clone() } else { self.me.name.clone() };
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
        let hosts: Vec<HostInfo> = peers.iter().filter(|p| p.advert.hosting && !p.advert.private).map(host_info).collect();

        self.follow_handover(&peers);
        if let Some(e) = self.handover_error.lock().unwrap().clone() {
            error = Some(e);
        }
        let handing_over = self.handover.lock().unwrap().as_ref().filter(|h| h.moved.is_none()).map(|h| h.name.clone());

        // Reconnect to the last host as soon as it shows up. A private one
        // is paired again with its saved code.
        let (reconnect, last, last_code) = {
            let p = self.prefs.lock().unwrap();
            (p.reconnect, p.last_host, p.last_code.clone())
        };
        if reconnect && self.link.lock().unwrap().is_none() {
            if let Some(h) = last.filter(|h| hosts.iter().any(|x| x.id == id_str(*h) && x.blocked.is_none())) {
                let _ = self.connect(h);
            } else if let (Some(h), Some(code)) = (last, last_code.clone())
                && peers.iter().any(|p| p.advert.id == h && p.advert.hosting && p.advert.private && !sources.contains(&h))
            {
                self.rejoin(h, code);
            }
        }

        // The listener we send to pressed Stop: disconnect at once instead of
        // waiting for it. (A listener that quit or dropped off the network
        // just disappears, and we keep waiting to reconnect.)
        let stopped = self.link.lock().unwrap().as_ref().is_some_and(|l| {
            peers.iter().any(|p| p.advert.id == l.host && !p.advert.hosting && p.advert.handover.is_none())
        });
        if stopped {
            self.forget_host();
        }

        self.drive_join(&peers);
        self.drive_upgrade(&peers);
        let mut buf = [0u8; 512];
        while let Ok((n, from)) = self.ctrl.recv_from(&mut buf) {
            let data = &buf[..n];
            if private::is_pair_reply(data) {
                self.on_pair_reply(data, from);
                continue;
            }
            if private::is_upgrade_reply(data) {
                self.on_upgrade_reply(data);
                continue;
            }
            let cipher = self.link.lock().unwrap().as_ref().and_then(|l| l.cipher.clone());
            let plain = match &cipher {
                Some(c) => c.open(data),
                None => Some(data.to_vec()),
            };
            if let Some(protocol::Packet::Status { .. }) = plain.as_deref().and_then(protocol::decode) {
                *self.last_ack.lock().unwrap() = Some(Instant::now());
                self.local_net_blocked.store(false, Relaxed);
            }
        }
        if let Some(l) = self.link.lock().unwrap().as_ref() {
            let mut last = self.last_want.lock().unwrap();
            if last.elapsed() >= WANT_EVERY {
                let port = l.back.as_ref().map_or(0, |b| b.shared.port.load(Relaxed));
                self.send_want(l.addr, port, l.hear, l.cipher.as_deref());
                *last = Instant::now();
            }
        }
        let mut lost_private: Option<u64> = None;
        let link = self.link.lock().unwrap().as_ref().map(|l| {
            let runs = [l.system.as_ref(), l.mic.as_ref()];
            let acked = self.last_ack.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(LINK_TIMEOUT_MS as u64));
            let alive = acked
                || runs.iter().flatten().any(|r| {
                    let last = r.shared.last_status_ms.load(Relaxed);
                    last > 0 && (r.started.elapsed().as_millis() as u32).saturating_sub(last) < LINK_TIMEOUT_MS
                });
            if !alive && l.cipher.is_some() && l.since.elapsed() > Duration::from_millis(LINK_TIMEOUT_MS as u64) {
                lost_private = Some(l.host);
            }
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

        // A private listener that restarted has forgotten our key: pair again.
        if let (Some(h), Some(code)) = (lost_private, last_code) {
            self.rejoin(h, code);
        }
        let joining = self.join.lock().unwrap().as_ref().is_some_and(|j| !j.quiet);
        let join_error = self.join_error.lock().unwrap().clone();

        // Only raise what matters for what this device is doing right now.
        let found = self.perms.get();
        let (mic_in_use, incoming) = {
            let host = self.host.lock().unwrap();
            let link = self.link.lock().unwrap();
            let serving_mic = host.as_ref().is_some_and(|h| h.returns.keys().any(|k| k.3));
            let sending_mic = link.as_ref().is_some_and(|l| l.mic.is_some());
            (serving_mic || sending_mic, host.is_some() || link.as_ref().is_some_and(|l| l.back.is_some()))
        };
        let issues: Vec<&'static str> = [
            (self.local_net_blocked.load(Relaxed), "localNetwork"),
            (found.audio_capture_denied, "audioCapture"),
            (found.mic_denied && mic_in_use, "microphone"),
            (found.firewall_missing && incoming, "firewall"),
        ]
        .into_iter()
        .filter_map(|(on, id)| on.then_some(id))
        .collect();

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
            issues,
            hosting,
            link,
            hosts,
            quality,
            output_lost,
            stats,
            devices,
            options: Options { reconnect, start_at_login, quiet_wifi, mute_speakers },
            error,
            handing_over,
            joining,
            join_error,
            edition: self.pro.edition(),
        }
    }

    /// macOS: make the Local Network prompt appear again (see
    /// `permissions::ask_local_network`).
    pub fn ask_local_network(&self) {
        let mut ips: Vec<IpAddr> = self.disco.peers.lock().unwrap().values().map(|p| p.ip).collect();
        if let Some(l) = self.link.lock().unwrap().as_ref() {
            ips.push(l.addr.ip());
        }
        crate::permissions::ask_local_network(ips);
    }

    /// Fix what the UI shows for an issue, then check again.
    pub fn fix_issue(&self, issue: &str) -> Result<()> {
        let result = crate::permissions::fix(issue);
        self.perms.recheck();
        result
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
        let prefs = Prefs { reconnect: false, quality: Quality::Highest, ..Default::default() };
        let engine = Engine::new(2, "test".into(), prefs, Arc::new(crate::pro::Free));
        engine.become_host().unwrap();
        assert!(engine.was_hosting());
        engine.shutdown();
        assert!(engine.was_hosting(), "quitting keeps the role");
        engine.become_host().unwrap();
        engine.stop_hosting();
        assert!(!engine.was_hosting(), "Stop forgets it");
    }

    /// The free edition can't listen privately or hand over.
    #[test]
    fn free_edition_keeps_pro_features_locked() {
        let prefs = Prefs { reconnect: false, ..Default::default() };
        let engine = Engine::new(3, "test".into(), prefs, Arc::new(crate::pro::Free));
        let e = engine.start_listening(None, Some("K7Q4MZ".into())).unwrap_err().to_string();
        assert!(e.contains("Aonin Pro"), "{e}");
        assert!(!engine.was_hosting(), "it didn't start listening publicly instead");
        let e = engine.hand_over(9).unwrap_err().to_string();
        assert!(e.contains("Aonin Pro"), "{e}");
        assert_eq!(engine.pro.edition(), Edition::Free);
        engine.shutdown();
    }

    /// A lock held across the snapshot once froze the UI: tick must return.
    #[test]
    fn tick_returns_and_never_deadlocks() {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let prefs = Prefs { quality: Quality::Cd, start_at_login: true, ..Default::default() };
            let engine = Engine::new(1, "test".into(), prefs, Arc::new(crate::pro::Free));
            let snaps: Vec<String> = (0..3).map(|_| engine.tick().quality.to_string()).collect();
            engine.shutdown();
            let _ = tx.send(snaps);
        });
        let snaps = rx.recv_timeout(Duration::from_secs(20)).expect("tick() hung");
        assert!(snaps.iter().all(|q| q == "cd"));
    }
}
