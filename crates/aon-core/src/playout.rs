//! Adaptive jitter buffer for one incoming stream.
//!
//! Blocks are written into a slot per sequence number as they arrive, in any
//! order, so a missing block never holds up the ones behind it. The output
//! callback reads slots in order and only gives up on a block at the moment it
//! is due to play. That leaves the whole buffer depth for the retransmit of a
//! lost block to arrive.
//!
//! Two loops keep the stream real time:
//!  - target latency grows by one block on every underrun and shrinks slowly
//!    while the network behaves, so it settles at what the Wi-Fi needs;
//!  - the resampling ratio is trimmed by up to ±0.2 % to hold the buffer at the
//!    target, which cancels clock drift between the two machines.

use crate::protocol::{BLOCK_FRAMES, BLOCK_SAMPLES, Block, SAMPLE_RATE};
use crate::resample::{Frame, Resampler};
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};

pub const MIN_TARGET: usize = 3 * BLOCK_FRAMES;
/// Start generous (20 ms) so Wi-Fi doesn't glitch while the target is learned;
/// it then shrinks on its own while the network behaves.
pub const START_TARGET: usize = 10 * BLOCK_FRAMES;
pub const MAX_TARGET: usize = 60 * BLOCK_FRAMES;
/// Slots in the buffer (~2 s). Blocks further ahead than this are dropped.
const SLOTS: usize = 1024;
const AHEAD_LIMIT: i32 = SLOTS as i32 - 64;
const MAX_TRIM: f64 = 0.002;
/// Smoothed excess over target (48 kHz frames) that triggers a one-off drop.
const SNAP_EXCESS: usize = 8 * BLOCK_FRAMES;

/// Signed distance from `b` to `a`, robust to u32 wrap-around.
fn diff(a: u32, b: u32) -> i32 {
    a.wrapping_sub(b) as i32
}

/// Counters shared between the network thread, the callback and the UI.
pub struct StreamStats {
    /// Target buffer depth in 48 kHz frames.
    pub target: AtomicUsize,
    /// Buffer depth seen by the last callback, in 48 kHz frames.
    pub level: AtomicUsize,
    pub underruns: AtomicU32,
    /// Blocks repaired by the redundant copy or a retransmit.
    pub recovered: AtomicU32,
    /// Blocks that never arrived in time and were papered over.
    pub concealed: AtomicU32,
    pub snaps: AtomicU32,
    /// Recent worst gap between packet arrivals in 48 kHz frames, measured
    /// by the network thread; the target never goes below it.
    pub gap: AtomicUsize,
    gain: AtomicU32,
}

impl StreamStats {
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Relaxed))
    }
    pub fn set_gain(&self, g: f32) {
        self.gain.store(g.to_bits(), Relaxed);
    }
    pub fn latency_ms(&self) -> f32 {
        self.level.load(Relaxed) as f32 * 1000.0 / SAMPLE_RATE as f32
    }
    pub fn target_ms(&self) -> f32 {
        self.target.load(Relaxed) as f32 * 1000.0 / SAMPLE_RATE as f32
    }
}

struct Slot {
    /// `1 << 32 | seq` once the block for `seq` is stored, 0 when empty.
    tag: AtomicU64,
    data: UnsafeCell<Block>,
}

struct Shared {
    slots: Box<[Slot]>,
    started: AtomicBool,
    /// Next sequence number to play. Written by the writer once to start,
    /// then only by the reader.
    play: AtomicU32,
    newest: AtomicU32,
}

// Safety: a slot's data is written only for seq in [play, play + AHEAD_LIMIT)
// and only while its tag doesn't already name that seq; the reader touches a
// slot's data only after seeing its tag (Acquire) for seq == play. The writer
// never reuses a slot until the reader has moved past it, so the two never
// access the same data concurrently.
unsafe impl Sync for Shared {}

impl Shared {
    fn tag(seq: u32) -> u64 {
        1 << 32 | seq as u64
    }
    fn slot(&self, seq: u32) -> &Slot {
        &self.slots[seq as usize % SLOTS]
    }
    fn has(&self, seq: u32) -> bool {
        self.slot(seq).tag.load(Acquire) == Self::tag(seq)
    }
}

/// Re-request a block if its retransmit hasn't shown up after this many
/// newer blocks (~6 ms).
const RENACK_AFTER: i32 = 3;
const MAX_NACKS: u8 = 3;

struct Nacked {
    seq: u32,
    /// Newest sequence number when we last asked.
    at: u32,
    tries: u8,
}

/// Network-thread end of a stream.
pub struct Writer {
    shared: Arc<Shared>,
    nacked: Vec<Nacked>,
}

/// Audio-callback end of a stream.
pub struct Playout {
    shared: Arc<Shared>,
    rs: Resampler,
    base_ratio: f64,
    err_lp: f64,
    primed: bool,
    since_underrun: usize,
    dst_rate: usize,
    cur: Block,
    pos: usize, // frames of `cur` already consumed; BLOCK_FRAMES = need next
    stats: Arc<StreamStats>,
}

/// Create the two ends of one incoming stream for an output running at `dst_rate`.
pub fn stream(dst_rate: u32) -> (Writer, Playout, Arc<StreamStats>) {
    let shared = Arc::new(Shared {
        slots: (0..SLOTS).map(|_| Slot { tag: AtomicU64::new(0), data: UnsafeCell::new([0.0; BLOCK_SAMPLES]) }).collect(),
        started: AtomicBool::new(false),
        play: AtomicU32::new(0),
        newest: AtomicU32::new(0),
    });
    let stats = Arc::new(StreamStats {
        target: AtomicUsize::new(START_TARGET),
        level: AtomicUsize::new(0),
        underruns: AtomicU32::new(0),
        recovered: AtomicU32::new(0),
        concealed: AtomicU32::new(0),
        snaps: AtomicU32::new(0),
        gap: AtomicUsize::new(0),
        gain: AtomicU32::new(1.0f32.to_bits()),
    });
    let base_ratio = SAMPLE_RATE as f64 / dst_rate as f64;
    let playout = Playout {
        shared: shared.clone(),
        rs: Resampler::new(base_ratio),
        base_ratio,
        err_lp: 0.0,
        primed: false,
        since_underrun: 0,
        dst_rate: dst_rate as usize,
        cur: [0.0; BLOCK_SAMPLES],
        pos: BLOCK_FRAMES,
        stats: stats.clone(),
    };
    (Writer { shared, nacked: Vec::new() }, playout, stats)
}

impl Writer {
    /// Store a received block. Returns true if it was new and still useful
    /// (not a duplicate and not already past its play time).
    pub fn insert(&mut self, seq: u32, block: &Block) -> bool {
        let sh = &*self.shared;
        if !sh.started.load(Relaxed) {
            sh.play.store(seq, Relaxed);
            sh.newest.store(seq, Relaxed);
            sh.started.store(true, Release);
        }
        let play = sh.play.load(Acquire);
        let d = diff(seq, play);
        if !(0..AHEAD_LIMIT).contains(&d) || sh.has(seq) {
            return false;
        }
        let slot = sh.slot(seq);
        // Safety: see `unsafe impl Sync for Shared`.
        unsafe { *slot.data.get() = *block };
        slot.tag.store(Shared::tag(seq), Release);
        if diff(seq, sh.newest.load(Relaxed)) > 0 {
            sh.newest.store(seq, Release);
        }
        true
    }

    /// The block for `seq` is already stored.
    pub fn has(&self, seq: u32) -> bool {
        self.shared.has(seq)
    }

    /// Blocks the redundant copy can no longer recover (a packet two or more
    /// blocks newer has arrived) that should be requested now: never asked
    /// for, or asked for `RENACK_AFTER` blocks ago without an answer (the
    /// retransmit can be lost too). At most `MAX_NACKS` requests per block.
    pub fn take_nacks(&mut self) -> Vec<u32> {
        let sh = &*self.shared;
        if !sh.started.load(Acquire) {
            return Vec::new();
        }
        let play = sh.play.load(Acquire);
        let newest = sh.newest.load(Acquire);
        self.nacked.retain(|n| diff(n.seq, play) >= 0 && !sh.has(n.seq));
        // The copy of `newest - 1` rides in the packet for `newest`.
        let span = diff(newest, play) - 1;
        let mut out = Vec::new();
        for d in 0..span.clamp(0, 256) {
            let seq = play.wrapping_add(d as u32);
            if sh.has(seq) {
                continue;
            }
            match self.nacked.iter_mut().find(|n| n.seq == seq) {
                None => self.nacked.push(Nacked { seq, at: newest, tries: 1 }),
                Some(n) if n.tries < MAX_NACKS && diff(newest, n.at) >= RENACK_AFTER => {
                    n.at = newest;
                    n.tries += 1;
                }
                Some(_) => continue,
            }
            out.push(seq);
        }
        out
    }
}

impl Playout {
    /// The network side is gone; the stream can be removed.
    pub fn finished(&self) -> bool {
        Arc::strong_count(&self.shared) == 1
    }

    /// Frames buffered: blocks from `play` to newest, plus what's left of `cur`.
    fn level(&self, play: u32) -> usize {
        let ahead = diff(self.shared.newest.load(Acquire), play) + 1;
        ahead.max(0) as usize * BLOCK_FRAMES + (BLOCK_FRAMES - self.pos)
    }

    /// Fetch the block for `play` into `cur`. Returns false on underrun.
    fn next_block(&mut self, play: &mut u32) -> bool {
        let sh = &*self.shared;
        if sh.has(*play) {
            // Safety: see `unsafe impl Sync for Shared`.
            self.cur = unsafe { *sh.slot(*play).data.get() };
        } else if diff(sh.newest.load(Acquire), *play) > 0 {
            // Later blocks are here, this one missed its deadline: repeat
            // the previous block fading out, much softer than a hard gap.
            for (i, f) in self.cur.chunks_exact_mut(2).enumerate() {
                let g = 1.0 - i as f32 / BLOCK_FRAMES as f32;
                f[0] *= g;
                f[1] *= g;
            }
            self.stats.concealed.fetch_add(1, Relaxed);
        } else {
            return false;
        }
        *play = play.wrapping_add(1);
        sh.play.store(*play, Release);
        self.pos = 0;
        true
    }

    /// Mix this stream into `out`. Never blocks or allocates.
    pub fn render_add(&mut self, out: &mut [Frame]) {
        let sh = &*self.shared;
        if !sh.started.load(Acquire) {
            return;
        }
        let mut play = sh.play.load(Relaxed);
        let level = self.level(play);
        self.stats.level.store(level, Relaxed);

        // The buffer must cover one whole callback plus the longest recent
        // gap between packet arrivals (Wi-Fi stalls), or it runs dry.
        let period = (out.len() as f64 * self.base_ratio).ceil() as usize;
        let gap = self.stats.gap.load(Relaxed);
        let floor = (period + gap.max(2 * BLOCK_FRAMES) + BLOCK_FRAMES).clamp(MIN_TARGET, MAX_TARGET);
        let mut target = self.stats.target.load(Relaxed).max(floor);

        if !self.primed {
            if level < target {
                self.stats.target.store(target, Relaxed);
                return;
            }
            self.primed = true;
            self.err_lp = 0.0;
        }

        self.since_underrun += out.len();
        if self.since_underrun > 10 * self.dst_rate && target > floor {
            target -= BLOCK_FRAMES / 2;
            self.since_underrun = 0;
        }
        self.stats.target.store(target, Relaxed);

        let err = (level as f64 - target as f64) / target as f64;
        self.err_lp += (err - self.err_lp) * 0.02;

        // Bound latency: the trim alone would take many seconds to work off a
        // large excess (startup burst, Wi-Fi stall then flood), so drop it once.
        if self.err_lp * target as f64 > SNAP_EXCESS as f64 {
            let skip = ((level - target) / BLOCK_FRAMES) as u32;
            play = play.wrapping_add(skip);
            sh.play.store(play, Release);
            self.err_lp = 0.0;
            self.stats.snaps.fetch_add(1, Relaxed);
        }

        let ratio = self.base_ratio * (1.0 + (self.err_lp * MAX_TRIM).clamp(-MAX_TRIM, MAX_TRIM));
        let gain = self.stats.gain();

        for o in out.iter_mut() {
            while self.rs.needs_input() {
                if self.pos == BLOCK_FRAMES && !self.next_block(&mut play) {
                    self.primed = false;
                    self.since_underrun = 0;
                    self.stats.target.store((target + BLOCK_FRAMES).min(MAX_TARGET), Relaxed);
                    self.stats.underruns.fetch_add(1, Relaxed);
                    return;
                }
                let i = self.pos * 2;
                self.rs.push([self.cur[i], self.cur[i + 1]]);
                self.pos += 1;
            }
            let f = self.rs.next(ratio);
            o[0] += f[0] * gain;
            o[1] += f[1] * gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blk(v: f32) -> Block {
        [v; BLOCK_SAMPLES]
    }

    fn render(po: &mut Playout, frames: usize) -> Vec<Frame> {
        let mut out = vec![[0.0f32; 2]; frames];
        po.render_add(&mut out);
        out
    }

    #[test]
    fn primes_then_plays_then_grows_on_underrun() {
        let (mut w, mut po, stats) = stream(SAMPLE_RATE);
        let blocks = START_TARGET / BLOCK_FRAMES;
        for s in 0..blocks as u32 - 1 {
            assert!(w.insert(100 + s, &blk(0.5)));
        }
        assert!(render(&mut po, 64).iter().all(|f| f[0] == 0.0), "must wait until primed");
        w.insert(100 + blocks as u32 - 1, &blk(0.5));
        assert!(render(&mut po, 64)[40][0] > 0.4, "plays once primed");
        // Drain everything in small callbacks: must underrun and raise the target.
        for _ in 0..20 {
            render(&mut po, 64);
        }
        assert_eq!(stats.underruns.load(Relaxed), 1);
        assert_eq!(stats.target.load(Relaxed), START_TARGET + BLOCK_FRAMES);
    }

    #[test]
    fn out_of_order_and_late_blocks() {
        let (mut w, mut po, stats) = stream(SAMPLE_RATE);
        let n = START_TARGET / BLOCK_FRAMES + 2;
        // Arrive in reverse order apart from the first; all must be accepted.
        w.insert(0, &blk(0.1));
        for s in (1..n as u32).rev() {
            assert!(w.insert(s, &blk(s as f32 * 0.1)));
        }
        assert!(!w.insert(3, &blk(9.0)), "duplicate rejected");
        render(&mut po, BLOCK_FRAMES * 3);
        assert!(!w.insert(0, &blk(9.0)), "already played, rejected");
        assert_eq!(stats.concealed.load(Relaxed), 0);
    }

    #[test]
    fn missing_block_is_concealed_only_at_its_deadline() {
        let (mut w, mut po, stats) = stream(SAMPLE_RATE);
        let n = START_TARGET / BLOCK_FRAMES + 2;
        for s in 0..n as u32 {
            if s != 2 {
                w.insert(s, &blk(0.5));
            }
        }
        assert_eq!(w.take_nacks(), vec![2]);
        assert!(w.take_nacks().is_empty(), "nacked once");
        render(&mut po, BLOCK_FRAMES); // plays block 0, block 2 still has time
        assert!(w.insert(2, &blk(0.5)), "retransmit accepted before deadline");
        render(&mut po, BLOCK_FRAMES * 3);
        assert_eq!(stats.concealed.load(Relaxed), 0);

        // Now one that never comes.
        let (mut w, mut po, stats) = stream(SAMPLE_RATE);
        for s in 0..n as u32 {
            if s != 2 {
                w.insert(s, &blk(0.5));
            }
        }
        render(&mut po, BLOCK_FRAMES * 4);
        assert_eq!(stats.concealed.load(Relaxed), 1);
        assert_eq!(stats.underruns.load(Relaxed), 0);
    }

    #[test]
    fn renacks_when_retransmit_is_lost_too() {
        let (mut w, _po, _) = stream(SAMPLE_RATE);
        for s in [0, 2, 3] {
            w.insert(s, &blk(0.5));
        }
        assert_eq!(w.take_nacks(), vec![1]);
        w.insert(4, &blk(0.5));
        w.insert(5, &blk(0.5));
        assert!(w.take_nacks().is_empty(), "too soon to ask again");
        w.insert(6, &blk(0.5));
        assert_eq!(w.take_nacks(), vec![1], "asked again after 3 blocks");
        for s in 7..20 {
            w.insert(s, &blk(0.5));
            w.take_nacks();
        }
        w.insert(20, &blk(0.5));
        assert!(w.take_nacks().is_empty(), "gives up after MAX_NACKS");
    }

    #[test]
    fn finished_after_writer_dropped() {
        let (w, po, _) = stream(SAMPLE_RATE);
        assert!(!po.finished());
        drop(w);
        assert!(po.finished());
    }
}
