//! UDP sender and hub receiver loops. Both run until their `stop` flag is
//! set and publish what the UI needs through a shared struct.

use anyhow::Result;
use crate::audio;
use aon_core::playout::{self, StreamStats};
use aon_core::protocol::{self, BLOCK_SAMPLES, Block, MAX_PACKET_BYTES, Os, Packet, SAMPLE_RATE, Source};
use aon_core::resample::Resampler;
use rtrb::Consumer;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, RandomState};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BLOCK_TIME: Duration = Duration::from_micros(1_000_000 * protocol::BLOCK_FRAMES as u64 / SAMPLE_RATE as u64);
/// Blocks kept for retransmission (~1 s).
const HISTORY: usize = 512;
/// If the capture device delivers nothing for this long (Windows loopback
/// goes quiet when nothing plays), send silence to keep the stream alive.
const CAPTURE_STALL: Duration = Duration::from_millis(40);
const SOURCE_TIMEOUT: Duration = Duration::from_secs(5);
const HELLO_EVERY: Duration = Duration::from_secs(1);

/// Lock-free f32 cell for numbers the UI polls.
#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed);
    }
    /// Read and reset to zero (for peaks collected between polls).
    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Relaxed))
    }
    pub fn raise(&self, v: f32) {
        let _ = self.0.try_update(Relaxed, Relaxed, |b| (v > f32::from_bits(b)).then_some(v.to_bits()));
    }
}

/// Who is sending. Announced to the hub in Hello packets.
#[derive(Clone)]
pub struct Identity {
    pub device: u64,
    pub name: String,
    pub os: Os,
}

/// Sender state the UI reads; `stop` ends the loop.
#[derive(Default)]
pub struct SendShared {
    pub stop: AtomicBool,
    /// Hub-side buffer depth reported by the last Status packet.
    pub buffer_ms: AtomicF32,
    pub peak: AtomicF32,
    /// Milliseconds since the sender started when the last Status arrived.
    pub last_status_ms: AtomicU32,
}

struct Outgoing {
    sock: UdpSocket,
    stream: u32,
    seq: u32,
    history: VecDeque<(u32, Box<Block>)>,
    buf: Vec<u8>,
    sent: u64,
    retransmitted: u64,
    /// Test aid: randomly drop this percentage of outgoing audio packets.
    drop_percent: u32,
    rng: u32,
}

impl Outgoing {
    fn emit(&mut self, block: Box<Block>) {
        let prev = self.history.back().filter(|(s, _)| s.wrapping_add(1) == self.seq).map(|(_, b)| &**b);
        protocol::encode_audio(&mut self.buf, self.stream, self.seq, &block, prev, false);
        self.send_lossy();
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back((self.seq, block));
        self.seq = self.seq.wrapping_add(1);
        self.sent += 1;
    }

    fn send_lossy(&mut self) {
        // xorshift32; good enough to simulate a lossy link
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        if self.rng % 100 >= self.drop_percent {
            let _ = self.sock.send(&self.buf);
        }
    }

    /// Answer retransmit requests and record the hub's status reports.
    fn serve(&mut self, shared: &SendShared, started: Instant) {
        let mut inbuf = [0u8; 2048];
        while let Ok(n) = self.sock.recv(&mut inbuf) {
            match protocol::decode(&inbuf[..n]) {
                Some(Packet::Nack { stream, seqs }) if stream == self.stream => {
                    for want in seqs {
                        if let Some((_, b)) = self.history.iter().find(|(s, _)| *s == want) {
                            protocol::encode_audio(&mut self.buf, self.stream, want, b, None, true);
                            self.send_lossy();
                            self.retransmitted += 1;
                        }
                    }
                }
                Some(Packet::Status { stream, buffer_ms }) if stream == self.stream => {
                    shared.buffer_ms.set(buffer_ms);
                    shared.last_status_ms.store(started.elapsed().as_millis() as u32, Relaxed);
                }
                _ => {}
            }
        }
    }
}

pub struct SendConfig {
    pub hub: SocketAddr,
    pub who: Identity,
    pub source: Source,
    pub drop_percent: u32,
    /// Print stats to stdout (CLI).
    pub verbose: bool,
}

pub fn run_sender(cfg: SendConfig, mut cap: Consumer<f32>, dev_rate: u32, shared: &SendShared) -> Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", 0))?;
    sock.connect(cfg.hub)?;
    sock.set_nonblocking(true)?;
    let stream = RandomState::new().hash_one(Instant::now()) as u32;
    if cfg.verbose {
        println!("Sending to {} (stream {stream:08x})", cfg.hub);
    }
    let mut net = Outgoing {
        sock,
        stream,
        seq: 0,
        history: VecDeque::with_capacity(HISTORY),
        buf: Vec::with_capacity(MAX_PACKET_BYTES),
        sent: 0,
        retransmitted: 0,
        drop_percent: cfg.drop_percent,
        rng: stream | 1,
    };
    let mut hello = Vec::new();
    protocol::encode_hello(&mut hello, stream, cfg.who.device, cfg.source, cfg.who.os, &cfg.who.name);

    let ratio = dev_rate as f64 / SAMPLE_RATE as f64;
    let mut rs = Resampler::new(ratio);
    let mut block: Box<Block> = Box::new([0.0; BLOCK_SAMPLES]);
    let mut fill = 0;
    let started = Instant::now();
    let mut last_input = started;
    let mut silence_due = started;
    let mut report = started;
    let mut last_hello = started - HELLO_EVERY;
    let mut peak = 0.0f32;
    let mut captured = 0u64;

    while !shared.stop.load(Relaxed) {
        net.serve(shared, started);

        let mut progressed = false;
        let mut chunk_peak = 0.0f32;
        'pull: loop {
            while rs.needs_input() {
                // Only take whole frames: the capture thread may be mid-push.
                if cap.slots() < 2 {
                    break 'pull;
                }
                let (l, r) = (cap.pop().unwrap(), cap.pop().unwrap());
                chunk_peak = chunk_peak.max(l.abs()).max(r.abs());
                captured += 1;
                rs.push([l, r]);
            }
            let f = rs.next(ratio);
            block[fill * 2] = f[0];
            block[fill * 2 + 1] = f[1];
            fill += 1;
            if fill == protocol::BLOCK_FRAMES {
                fill = 0;
                net.emit(std::mem::replace(&mut block, Box::new([0.0; BLOCK_SAMPLES])));
            }
            progressed = true;
        }
        shared.peak.raise(chunk_peak);
        peak = peak.max(chunk_peak);

        let now = Instant::now();
        if now - last_hello >= HELLO_EVERY {
            let _ = net.sock.send(&hello);
            last_hello = now;
        }
        if progressed {
            last_input = now;
            silence_due = now;
        } else if now - last_input > CAPTURE_STALL {
            while silence_due <= now {
                net.emit(Box::new([0.0; BLOCK_SAMPLES]));
                silence_due += BLOCK_TIME;
            }
        }

        if cfg.verbose && now - report > Duration::from_secs(5) {
            println!(
                "sent {} blocks, retransmitted {}, captured {captured} frames, input peak {}",
                net.sent,
                net.retransmitted,
                dbfs(peak)
            );
            captured = 0;
            peak = 0.0;
            report = now;
        }
        if !progressed {
            std::thread::sleep(Duration::from_micros(500));
        }
    }
    Ok(())
}

/// One incoming stream as the UI sees it.
#[derive(Clone)]
pub struct StreamView {
    pub addr: SocketAddr,
    pub stream: u32,
    /// From the stream's Hello; `None` until one arrives (e.g. an old sender).
    pub who: Option<(Identity, Source)>,
    pub stats: Arc<StreamStats>,
    /// Peak level since the previous publish.
    pub peak: f32,
}

/// Hub state the UI reads; `stop` ends the loop.
#[derive(Default)]
pub struct HubShared {
    pub stop: AtomicBool,
    pub streams: Mutex<Vec<StreamView>>,
    /// The output device is gone and the hub is waiting to reopen it.
    pub output_lost: AtomicBool,
    /// Name of the device currently playing the mix.
    pub output: Mutex<String>,
    /// Ask the hub to reopen its output (e.g. the user picked another device).
    pub reopen: AtomicBool,
    /// The UDP port the hub actually listens on (useful when bound to 0).
    pub port: AtomicU16,
    /// Clients asking to hear this device, by (address, device id).
    pub wants: Mutex<HashMap<(IpAddr, u64), Want>>,
}

/// A client's standing request to have this host's audio streamed back.
#[derive(Clone, Copy, Debug)]
pub struct Want {
    pub port: u16,
    pub system: bool,
    pub mic: bool,
    pub at: Instant,
}

struct Incoming {
    writer: playout::Writer,
    stats: Arc<StreamStats>,
    who: Option<(Identity, Source)>,
    last_seen: Instant,
    /// Highest sample magnitude received since the last publish / report.
    peak: f32,
    /// Decaying envelope of the gap between packet arrivals, in seconds.
    gap_env: f64,
    /// Longest gap since the last report, in seconds.
    worst_gap: f64,
}

/// How fast a past Wi-Fi stall stops holding the buffer up.
const GAP_HALF_LIFE: f64 = 20.0;

impl Incoming {
    /// Wi-Fi delivers in bursts with stalls of tens of ms. Holding the
    /// buffer above the worst recent stall prevents the underrun instead of
    /// learning from it one glitch at a time.
    fn track_gap(&mut self, gap: f64) {
        let gap = gap.min(0.1); // a sender pause is not network jitter
        self.gap_env = gap.max(self.gap_env * 0.5f64.powf(gap / GAP_HALF_LIFE));
        self.worst_gap = self.worst_gap.max(gap);
        self.stats.gap.store((self.gap_env * SAMPLE_RATE as f64) as usize, Relaxed);
    }
}

pub fn dbfs(peak: f32) -> String {
    if peak > 0.0 { format!("{:.1} dBFS", 20.0 * peak.log10()) } else { "silent".into() }
}

/// Give an incoming stream a fresh jitter buffer playing on `output`.
fn attach(src: &mut Incoming, output: Option<&audio::Output>) {
    let rate = output.map_or(SAMPLE_RATE, |o| o.rate);
    let (writer, po, stats) = playout::stream(rate);
    stats.gap.store(src.stats.gap.load(Relaxed), Relaxed);
    stats.set_gain(src.stats.gain());
    if let Some(o) = output {
        let _ = o.tx.send(po);
    }
    src.writer = writer;
    src.stats = stats;
}

/// Run the hub until `shared.stop`. `open` opens the output; it is called
/// again whenever the device fails, `shared.reopen` is set, or, with
/// `follow_default`, the system default output changes.
pub fn run_hub(
    port: u16,
    mut open: impl FnMut() -> Result<audio::Output>,
    follow_default: bool,
    shared: &HubShared,
    verbose: bool,
) -> Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", port))?;
    sock.set_read_timeout(Some(Duration::from_millis(5)))?;
    let port = sock.local_addr()?.port();
    shared.port.store(port, Relaxed);
    if verbose {
        println!("Hub listening on UDP port {port}");
    }

    let mut sources: HashMap<(SocketAddr, u32), Incoming> = HashMap::new();
    let mut buf = [0u8; 2048];
    let mut out = Vec::new();
    let mut report = Instant::now();
    let mut publish = Instant::now();
    let mut status = Instant::now();
    let mut output: Option<audio::Output> = None;
    let mut retry_at = Instant::now();
    let mut default_check = Instant::now();

    let join = |sources: &mut HashMap<(SocketAddr, u32), Incoming>, addr: SocketAddr, stream: u32, output: Option<&audio::Output>| {
        sources.entry((addr, stream)).or_insert_with(|| {
            if verbose {
                println!("+ {addr} joined (stream {stream:08x})");
            }
            let (writer, _, stats) = playout::stream(SAMPLE_RATE);
            let mut src = Incoming { writer, stats, who: None, last_seen: Instant::now(), peak: 0.0, gap_env: 0.0, worst_gap: 0.0 };
            attach(&mut src, output);
            src
        });
    };

    while !shared.stop.load(Relaxed) {
        // Keep an output open: reopen after a device error or unplug, on
        // request, or when the system default moves to another device.
        let now = Instant::now();
        let mut stale = output.as_ref().is_none_or(|o| o.failed.load(Relaxed)) || shared.reopen.swap(false, Relaxed);
        if follow_default && !stale && now - default_check > Duration::from_secs(2) {
            default_check = now;
            let current = audio::device_names().default_output;
            stale = output.as_ref().zip(current).is_some_and(|(o, d)| o.device != d);
        }
        if stale && now >= retry_at {
            output = None;
            match open() {
                Ok(o) => {
                    *shared.output.lock().unwrap() = o.device.clone();
                    shared.output_lost.store(false, Relaxed);
                    for src in sources.values_mut() {
                        attach(src, Some(&o));
                    }
                    output = Some(o);
                }
                Err(e) => {
                    if verbose {
                        println!("output unavailable: {e}");
                    }
                    shared.output_lost.store(true, Relaxed);
                    retry_at = now + Duration::from_secs(1);
                }
            }
        }

        if let Ok((n, addr)) = sock.recv_from(&mut buf) {
            match protocol::decode(&buf[..n]) {
                Some(Packet::Audio { stream, seq, blocks, retransmit }) => {
                    join(&mut sources, addr, stream, output.as_ref());
                    let src = sources.get_mut(&(addr, stream)).unwrap();
                    let now = Instant::now();
                    if !retransmit {
                        src.track_gap((now - src.last_seen).as_secs_f64());
                    }
                    src.last_seen = now;
                    for (i, b) in blocks.iter().enumerate() {
                        let fresh = src.writer.insert(seq.wrapping_sub(i as u32), b);
                        if fresh {
                            src.peak = b.iter().fold(src.peak, |p, s| p.max(s.abs()));
                            if i == 1 || retransmit {
                                src.stats.recovered.fetch_add(1, Relaxed);
                            }
                        }
                    }
                    let missing = src.writer.take_nacks();
                    if !missing.is_empty() {
                        protocol::encode_nack(&mut out, stream, &missing);
                        let _ = sock.send_to(&out, addr);
                    }
                }
                Some(Packet::Want { device, port, system, mic }) => {
                    let want = Want { port, system, mic, at: Instant::now() };
                    shared.wants.lock().unwrap().insert((addr.ip(), device), want);
                    // Acknowledge, so a client that sends nothing still knows we're here.
                    protocol::encode_status(&mut out, 0, 0.0);
                    let _ = sock.send_to(&out, addr);
                }
                Some(Packet::Hello { stream, device, source, os, name }) => {
                    join(&mut sources, addr, stream, output.as_ref());
                    let src = sources.get_mut(&(addr, stream)).unwrap();
                    src.who = Some((Identity { device, name, os }, source));
                }
                _ => {}
            }
        }

        let now = Instant::now();
        sources.retain(|(addr, _), s| {
            let alive = now - s.last_seen < SOURCE_TIMEOUT;
            if !alive && verbose {
                println!("- {addr} left");
            }
            alive
        });

        if now - status >= Duration::from_secs(1) {
            for ((addr, stream), s) in &sources {
                protocol::encode_status(&mut out, *stream, s.stats.latency_ms());
                let _ = sock.send_to(&out, addr);
            }
            status = now;
        }

        if now - publish >= Duration::from_millis(50) {
            let views = sources
                .iter_mut()
                .map(|((addr, stream), s)| {
                    let peak = std::mem::take(&mut s.peak);
                    StreamView { addr: *addr, stream: *stream, who: s.who.clone(), stats: s.stats.clone(), peak }
                })
                .collect();
            *shared.streams.lock().unwrap() = views;
            publish = now;
        }

        if verbose && now - report > Duration::from_secs(2) {
            for ((addr, _), s) in sources.iter_mut() {
                let st = &s.stats;
                println!(
                    "  {addr}: buffer {:.1} ms (target {:.1} ms), recovered {}, concealed {}, underruns {}, snaps {}, worst gap {:.1} ms",
                    st.latency_ms(),
                    st.target_ms(),
                    st.recovered.load(Relaxed),
                    st.concealed.load(Relaxed),
                    st.underruns.load(Relaxed),
                    st.snaps.load(Relaxed),
                    s.worst_gap * 1000.0,
                );
                s.worst_gap = 0.0;
            }
            report = now;
        }
    }
    Ok(())
}
