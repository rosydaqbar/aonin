//! UDP sender and hub receiver loops. Both run until their `stop` flag is
//! set and publish what the UI needs through a shared struct.

use anyhow::Result;
use crate::audio;
use crate::private::{self, Cipher};
use crate::pro::PrivateHubOps;
use aon_core::playout::{self, StreamStats};
use aon_core::protocol::{self, BLOCK_SAMPLES, Block, Codec, MAX_PACKET_BYTES, OPUS_FRAME_BLOCKS, OPUS_FRAME_FRAMES, Os, Packet, Quality, SAMPLE_RATE, Source};
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
/// An open output that hasn't asked for audio this long is stuck.
const OUTPUT_STALL: Duration = Duration::from_millis(1500);
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
    /// Send to this listener from now on (handover). Capture keeps running,
    /// so the switch costs no audio on this side.
    pub retarget: Mutex<Option<SocketAddr>>,
}

struct Outgoing {
    sock: UdpSocket,
    stream: u32,
    seq: u32,
    codec: Codec,
    /// Opus only: the encoder, samples waiting for a full frame (and the
    /// frame's first block), and sent frames kept for retransmission.
    opus: Option<opus::Encoder>,
    pending: Vec<f32>,
    pending_seq: u32,
    frames: VecDeque<(u32, Vec<u8>)>,
    history: VecDeque<(u32, Box<Block>)>,
    buf: Vec<u8>,
    sent: u64,
    retransmitted: u64,
    /// Test aid: randomly drop this percentage of outgoing audio packets.
    drop_percent: u32,
    rng: u32,
    /// Private session: every packet both ways is sealed with this key.
    cipher: Option<Arc<Cipher>>,
    sealed: Vec<u8>,
}

/// Send `plain`, sealed first when the session is private.
fn tx(sock: &UdpSocket, to: Option<SocketAddr>, cipher: Option<&Cipher>, plain: &[u8], scratch: &mut Vec<u8>) {
    let bytes = match cipher {
        Some(c) => {
            c.seal(plain, scratch);
            &scratch[..]
        }
        None => plain,
    };
    let _ = match to {
        Some(addr) => sock.send_to(bytes, addr),
        None => sock.send(bytes),
    };
}

/// The plain packet in `data`: opened with `cipher` in a private session
/// (`None` if it isn't sealed with it), as is otherwise.
fn plain(data: &[u8], cipher: Option<&Cipher>) -> Option<Vec<u8>> {
    match cipher {
        Some(c) => c.open(data),
        None => Some(data.to_vec()),
    }
}

impl Outgoing {
    fn emit(&mut self, block: Box<Block>) {
        if self.opus.is_some() {
            return self.emit_opus(block);
        }
        let prev = self.history.back().filter(|(s, _)| s.wrapping_add(1) == self.seq).map(|(_, b)| &**b);
        protocol::encode_audio(&mut self.buf, self.codec, self.stream, self.seq, &block, prev, false);
        self.send_lossy();
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back((self.seq, block));
        self.seq = self.seq.wrapping_add(1);
        self.sent += 1;
    }

    /// Collect blocks into 10 ms frames, encode, send with the previous frame
    /// as the redundant copy.
    fn emit_opus(&mut self, block: Box<Block>) {
        if self.pending.is_empty() {
            self.pending_seq = self.seq;
        }
        self.pending.extend_from_slice(&block[..]);
        self.seq = self.seq.wrapping_add(1);
        self.sent += 1;
        if self.pending.len() < OPUS_FRAME_FRAMES * 2 {
            return;
        }
        let mut out = [0u8; 1500];
        let n = match self.opus.as_mut().unwrap().encode_float(&self.pending, &mut out) {
            Ok(n) => n,
            Err(_) => 0,
        };
        self.pending.clear();
        if n == 0 {
            return;
        }
        let first = self.pending_seq;
        let prev = self
            .frames
            .back()
            .filter(|(s, _)| s.wrapping_add(OPUS_FRAME_BLOCKS as u32) == first)
            .map(|(_, f)| f.as_slice());
        protocol::encode_opus(&mut self.buf, self.stream, first, &out[..n], prev, false);
        self.send_lossy();
        if self.frames.len() == HISTORY / OPUS_FRAME_BLOCKS {
            self.frames.pop_front();
        }
        self.frames.push_back((first, out[..n].to_vec()));
    }

    fn send_lossy(&mut self) {
        // xorshift32; good enough to simulate a lossy link
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        if self.rng % 100 >= self.drop_percent {
            tx(&self.sock, None, self.cipher.as_deref(), &self.buf, &mut self.sealed);
        }
    }

    /// Answer retransmit requests and record the hub's status reports.
    fn serve(&mut self, shared: &SendShared, started: Instant) {
        let mut inbuf = [0u8; 2048];
        while let Ok(n) = self.sock.recv(&mut inbuf) {
            let Some(data) = plain(&inbuf[..n], self.cipher.as_deref()) else { continue };
            match protocol::decode(&data) {
                Some(Packet::Nack { stream, seqs }) if stream == self.stream && self.opus.is_some() => {
                    // Resend each frame that covers a missing block, once.
                    let mut firsts: Vec<u32> = Vec::new();
                    for want in seqs {
                        let hit = self.frames.iter().find(|(s, _)| want.wrapping_sub(*s) < OPUS_FRAME_BLOCKS as u32);
                        if let Some((s, _)) = hit {
                            if !firsts.contains(s) {
                                firsts.push(*s);
                            }
                        }
                    }
                    for first in firsts {
                        let frame = self.frames.iter().find(|(s, _)| *s == first).map(|(_, f)| f.clone()).unwrap();
                        protocol::encode_opus(&mut self.buf, self.stream, first, &frame, None, true);
                        self.send_lossy();
                        self.retransmitted += 1;
                    }
                }
                Some(Packet::Nack { stream, seqs }) if stream == self.stream => {
                    for want in seqs {
                        if let Some((_, b)) = self.history.iter().find(|(s, _)| *s == want) {
                            protocol::encode_audio(&mut self.buf, self.codec, self.stream, want, b, None, true);
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
    pub quality: Quality,
    pub drop_percent: u32,
    /// Print stats to stdout (CLI).
    pub verbose: bool,
    /// Private session key shared with the hub; `None` sends in the clear.
    pub cipher: Option<Arc<Cipher>>,
}

pub fn run_sender(cfg: SendConfig, mut cap: Consumer<f32>, dev_rate: u32, shared: &SendShared) -> Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", 0))?;
    sock.connect(cfg.hub)?;
    sock.set_nonblocking(true)?;
    let stream = RandomState::new().hash_one(Instant::now()) as u32;
    if cfg.verbose {
        println!("Sending to {} (stream {stream:08x})", cfg.hub);
    }
    let opus = match cfg.quality.opus_bitrate() {
        Some(bits) => {
            let mut enc = opus::Encoder::new(SAMPLE_RATE, opus::Channels::Stereo, opus::Application::LowDelay)?;
            enc.set_bitrate(opus::Bitrate::Bits(bits))?;
            Some(enc)
        }
        None => None,
    };
    let mut net = Outgoing {
        sock,
        stream,
        seq: 0,
        codec: cfg.quality.codec(),
        opus,
        pending: Vec::with_capacity(OPUS_FRAME_FRAMES * 2),
        pending_seq: 0,
        frames: VecDeque::new(),
        history: VecDeque::with_capacity(HISTORY),
        buf: Vec::with_capacity(MAX_PACKET_BYTES),
        sent: 0,
        retransmitted: 0,
        drop_percent: cfg.drop_percent,
        rng: stream | 1,
        cipher: cfg.cipher.clone(),
        sealed: Vec::with_capacity(MAX_PACKET_BYTES + private::OVERHEAD),
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
        if let Some(to) = shared.retarget.lock().unwrap().take() {
            // Same stream and sequence: the new listener starts it like any
            // stream it hasn't heard before. Introduce ourselves at once.
            if net.sock.connect(to).is_ok() {
                last_hello = now - HELLO_EVERY;
            }
        }
        if now - last_hello >= HELLO_EVERY {
            tx(&net.sock, None, net.cipher.as_deref(), &hello, &mut net.sealed);
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
    /// Private session keys by device address: what arrives sealed from an
    /// address is opened with its key, and answered sealed with it.
    pub keys: Mutex<HashMap<IpAddr, Arc<Cipher>>>,
    /// Set while private: the code and the name shown to devices that pair.
    /// Plain packets are then refused.
    pub private: Mutex<Option<PrivateHub>>,
    /// Devices that were connected when the listener turned private, by
    /// address and when. They may ask for the code (see `private::Upgrade`).
    pub grandfather: Mutex<HashMap<IpAddr, Instant>>,
}

/// How long a device connected before the listener turned private may ask
/// for the code.
pub const GRANDFATHER: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct PrivateHub {
    pub code: String,
    pub name: String,
    /// Answers pairings (Aonin Pro). Without it the hub stays private but
    /// lets nobody new in.
    pub ops: Option<Arc<dyn PrivateHubOps>>,
}

/// Pairing attempts one address may make per minute. The code has 30 bits,
/// so guessing it this way would take thousands of years.
const PAIR_TRIES_PER_MINUTE: u32 = 10;

/// A client's standing request to have this host's audio streamed back.
#[derive(Clone, Copy, Debug)]
pub struct Want {
    pub port: u16,
    pub system: bool,
    pub mic: bool,
    pub at: Instant,
}

struct Incoming {
    /// Opus streams only, created on the first Opus packet.
    opus: Option<opus::Decoder>,
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
    /// Private session: replies to this stream are sealed with its key.
    key: Option<Arc<Cipher>>,
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
    let mut sealed = Vec::new();
    let mut pair_tries: HashMap<IpAddr, (Instant, u32)> = HashMap::new();
    let mut answered: HashMap<IpAddr, (Vec<u8>, Vec<u8>)> = HashMap::new();
    let mut upgraded: HashMap<IpAddr, (Vec<u8>, Vec<u8>)> = HashMap::new();
    let mut report = Instant::now();
    let mut publish = Instant::now();
    let mut status = Instant::now();
    let mut output: Option<audio::Output> = None;
    let mut retry_at = Instant::now();
    let mut default_check = Instant::now();
    // Output callbacks seen, and when that count last moved.
    let mut calls = (0u32, Instant::now());
    // The last output opened fine but never played.
    let mut stalled = false;

    let join = |sources: &mut HashMap<(SocketAddr, u32), Incoming>, addr: SocketAddr, stream: u32, output: Option<&audio::Output>| {
        sources.entry((addr, stream)).or_insert_with(|| {
            if verbose {
                println!("+ {addr} joined (stream {stream:08x})");
            }
            let (writer, _, stats) = playout::stream(SAMPLE_RATE);
            let mut src = Incoming { opus: None, writer, stats, who: None, last_seen: Instant::now(), peak: 0.0, gap_env: 0.0, worst_gap: 0.0, key: None };
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
                    // After a stall, stay "lost" until this one really plays.
                    shared.output_lost.store(stalled, Relaxed);
                    calls = (0, now);
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

        // An output that stops asking for audio is as good as gone: reopen
        // it, and say so if it stays silent.
        if let Some(o) = output.as_ref() {
            let n = o.calls.load(Relaxed);
            if n != calls.0 {
                calls = (n, now);
                if stalled {
                    stalled = false;
                    shared.output_lost.store(false, Relaxed);
                }
            } else if now - calls.1 > OUTPUT_STALL && !o.failed.load(Relaxed) {
                if verbose {
                    println!("output stalled: {} isn't asking for audio", o.device);
                }
                stalled = true;
                shared.output_lost.store(true, Relaxed);
                o.failed.store(true, Relaxed);
                retry_at = now + Duration::from_secs(2);
            }
        }

        // What arrived, opened if sealed, with the key to answer it with.
        let got = match sock.recv_from(&mut buf) {
            Ok((n, addr)) => {
                let raw = &buf[..n];
                if private::is_sealed(raw) {
                    let key = shared.keys.lock().unwrap().get(&addr.ip()).cloned();
                    key.and_then(|k| k.open(raw).map(|p| (p, Some(k), addr)))
                } else if private::is_pair_start(raw) {
                    let session = shared.private.lock().unwrap().clone();
                    if let Some(s) = session {
                        let now = Instant::now();
                        let tries = pair_tries.entry(addr.ip()).or_insert((now, 0));
                        if now - tries.0 > Duration::from_secs(60) {
                            *tries = (now, 0);
                        }
                        // A resend of the same PairStart gets the same answer:
                        // a new key would undo the one the sender just got.
                        if let Some((start, reply)) = answered.get(&addr.ip())
                            && start.as_slice() == raw
                        {
                            let _ = sock.send_to(reply, addr);
                        } else {
                            tries.1 += 1;
                            if tries.1 <= PAIR_TRIES_PER_MINUTE
                                && let Some(ops) = &s.ops
                                && let Some((reply, key)) = ops.answer(raw, &s.code, &s.name)
                            {
                                // With a wrong code this is a key the sender
                                // doesn't have, so nothing it sends will open.
                                shared.keys.lock().unwrap().insert(addr.ip(), Arc::new(key));
                                let _ = sock.send_to(&reply, addr);
                                answered.insert(addr.ip(), (raw.to_vec(), reply));
                            }
                        }
                    }
                    None
                } else if private::is_upgrade_request(raw) {
                    let session = shared.private.lock().unwrap().clone();
                    let allowed = shared.grandfather.lock().unwrap().get(&addr.ip()).is_some_and(|t| t.elapsed() < GRANDFATHER);
                    if let (Some(s), true) = (session, allowed) {
                        if let Some((req, reply)) = upgraded.get(&addr.ip())
                            && req.as_slice() == raw
                        {
                            let _ = sock.send_to(reply, addr);
                        } else if let Some(reply) = s.ops.as_ref().and_then(|ops| ops.answer_upgrade(raw, &s.code)) {
                            let _ = sock.send_to(&reply, addr);
                            upgraded.insert(addr.ip(), (raw.to_vec(), reply));
                        }
                    }
                    None
                } else if shared.private.lock().unwrap().is_some() {
                    None
                } else {
                    Some((raw.to_vec(), None, addr))
                }
            }
            Err(_) => None,
        };

        if let Some((data, key, addr)) = got {
            match protocol::decode(&data) {
                Some(Packet::Audio { stream, seq, blocks, retransmit }) => {
                    join(&mut sources, addr, stream, output.as_ref());
                    let src = sources.get_mut(&(addr, stream)).unwrap();
                    src.key = key.clone();
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
                        tx(&sock, Some(addr), src.key.as_deref(), &out, &mut sealed);
                    }
                }
                Some(Packet::Opus { stream, seq, frames, retransmit }) => {
                    join(&mut sources, addr, stream, output.as_ref());
                    let src = sources.get_mut(&(addr, stream)).unwrap();
                    src.key = key.clone();
                    let now = Instant::now();
                    if !retransmit {
                        src.track_gap((now - src.last_seen).as_secs_f64());
                    }
                    src.last_seen = now;
                    if src.opus.is_none() {
                        src.opus = opus::Decoder::new(SAMPLE_RATE, opus::Channels::Stereo).ok();
                    }
                    let mut pcm = vec![0f32; OPUS_FRAME_FRAMES * 2];
                    for (i, frame) in frames.iter().enumerate() {
                        let first = seq.wrapping_sub((i * OPUS_FRAME_BLOCKS) as u32);
                        // Decoding a frame twice would disturb the decoder state.
                        if src.writer.has(first) {
                            continue;
                        }
                        let Some(dec) = src.opus.as_mut() else { break };
                        let Ok(n) = dec.decode_float(frame, &mut pcm, false) else { continue };
                        let mut fresh = false;
                        for (k, chunk) in pcm[..n * 2].chunks_exact(BLOCK_SAMPLES).enumerate() {
                            let mut b = [0f32; BLOCK_SAMPLES];
                            b.copy_from_slice(chunk);
                            src.peak = b.iter().fold(src.peak, |p, s| p.max(s.abs()));
                            fresh |= src.writer.insert(first.wrapping_add(k as u32), &b);
                        }
                        if fresh && (i == 1 || retransmit) {
                            src.stats.recovered.fetch_add(1, Relaxed);
                        }
                    }
                    let missing = src.writer.take_nacks();
                    if !missing.is_empty() {
                        protocol::encode_nack(&mut out, stream, &missing);
                        tx(&sock, Some(addr), src.key.as_deref(), &out, &mut sealed);
                    }
                }
                Some(Packet::Want { device, port, system, mic }) => {
                    let want = Want { port, system, mic, at: Instant::now() };
                    shared.wants.lock().unwrap().insert((addr.ip(), device), want);
                    // Acknowledge, so a client that sends nothing still knows we're here.
                    protocol::encode_status(&mut out, 0, 0.0);
                    tx(&sock, Some(addr), key.as_deref(), &out, &mut sealed);
                }
                Some(Packet::Hello { stream, device, source, os, name }) => {
                    join(&mut sources, addr, stream, output.as_ref());
                    let src = sources.get_mut(&(addr, stream)).unwrap();
                    src.who = Some((Identity { device, name, os }, source));
                    src.key = key;
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
                tx(&sock, Some(*addr), s.key.as_deref(), &out, &mut sealed);
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
