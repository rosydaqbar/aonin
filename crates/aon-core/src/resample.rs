//! Pull-based windowed-sinc resampler for stereo frames.
//!
//! Used for two things: converting between device rates and the 48 kHz wire
//! rate, and absorbing clock drift between machines by nudging the ratio a
//! few hundred ppm. At a ratio of exactly 1.0 it passes samples through
//! bit-exact, because the sinc kernel at integer offsets is a pure delta.

use std::collections::VecDeque;

const HALF: usize = 16; // taps on each side
const TAPS: usize = HALF * 2;
const PHASES: usize = 512;

pub type Frame = [f32; 2];

pub struct Resampler {
    table: Vec<[f32; TAPS]>, // PHASES + 1 rows
    hist: VecDeque<Frame>,
    pos: f64,
}

impl Resampler {
    /// `nominal_ratio` = input frames consumed per output frame
    /// (input rate / output rate). It sets the anti-aliasing cutoff.
    pub fn new(nominal_ratio: f64) -> Self {
        let fc = (1.0 / nominal_ratio).min(1.0);
        let table = (0..=PHASES)
            .map(|p| {
                let frac = p as f64 / PHASES as f64;
                let mut row = [0.0f64; TAPS];
                for (j, h) in row.iter_mut().enumerate() {
                    let x = (j as f64 - (HALF as f64 - 1.0)) - frac;
                    let sinc = if x == 0.0 { 1.0 } else { (std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * fc * x) };
                    let n = (x + HALF as f64) / TAPS as f64; // 0..1 across the window
                    let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * n).cos() + 0.08 * (4.0 * std::f64::consts::PI * n).cos();
                    *h = sinc * w;
                    // sin(pi*k) is ~1e-17, not 0; flush it so phase 0 is an exact delta.
                    if h.abs() < 1e-12 {
                        *h = 0.0;
                    }
                }
                let sum: f64 = row.iter().sum();
                row.map(|h| (h / sum) as f32)
            })
            .collect();
        let mut hist = VecDeque::with_capacity(TAPS * 4);
        hist.extend(std::iter::repeat_n([0.0; 2], HALF - 1));
        Self { table, hist, pos: (HALF - 1) as f64 }
    }

    /// True if another input frame must be pushed before `next` can run.
    pub fn needs_input(&self) -> bool {
        self.hist.len() <= self.pos as usize + HALF
    }

    pub fn push(&mut self, frame: Frame) {
        self.hist.push_back(frame);
    }

    /// Produce one output frame, then advance by `ratio` input frames.
    pub fn next(&mut self, ratio: f64) -> Frame {
        debug_assert!(!self.needs_input());
        let i0 = self.pos as usize;
        let frac = (self.pos - i0 as f64) * PHASES as f64;
        let p = frac as usize;
        let t = (frac - p as f64) as f32;
        let (a, b) = (&self.table[p], &self.table[p + 1]);
        let mut out = [0.0f32; 2];
        let base = i0 + 1 - HALF;
        for j in 0..TAPS {
            let h = a[j] + (b[j] - a[j]) * t;
            let s = self.hist[base + j];
            out[0] += s[0] * h;
            out[1] += s[1] * h;
        }
        self.pos += ratio;
        while self.pos as usize >= HALF {
            self.hist.pop_front();
            self.pos -= 1.0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(rs: &mut Resampler, input: &[Frame], ratio: f64) -> Vec<Frame> {
        let mut out = Vec::new();
        let mut it = input.iter();
        loop {
            while rs.needs_input() {
                match it.next() {
                    Some(&f) => rs.push(f),
                    None => return out,
                }
            }
            out.push(rs.next(ratio));
        }
    }

    #[test]
    fn unity_ratio_is_bit_exact() {
        let input: Vec<Frame> = (0..1000).map(|i| [(i as f32 * 0.37).sin(), (i as f32 * 0.11).cos() * 0.5]).collect();
        let out = run(&mut Resampler::new(1.0), &input, 1.0);
        // The zero history sits behind the read position, so there is no delay.
        assert_eq!(out.len(), input.len() - HALF);
        for (o, i) in out.iter().zip(&input) {
            assert_eq!(o, i);
        }
    }

    #[test]
    fn converts_44k1_to_48k_accurately() {
        let (fin, fout, tone) = (44_100.0, 48_000.0, 1_000.0);
        let input: Vec<Frame> = (0..44_100).map(|i| {
            let s = (2.0 * std::f64::consts::PI * tone * i as f64 / fin).sin() as f32 * 0.8;
            [s, -s]
        }).collect();
        let ratio = fin / fout;
        let out = run(&mut Resampler::new(ratio), &input, ratio);
        let mut worst = 0.0f32;
        for (n, o) in out.iter().enumerate().skip(200).take(40_000) {
            let t = n as f64 * ratio;
            let want = (2.0 * std::f64::consts::PI * tone * t / fin).sin() as f32 * 0.8;
            worst = worst.max((o[0] - want).abs()).max((o[1] + want).abs());
        }
        assert!(worst < 1e-3, "max error {worst}");
    }
}
