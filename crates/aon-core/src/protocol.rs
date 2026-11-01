//! Wire format. Everything travels as UDP datagrams on the local network.
//!
//! Audio is uncompressed 24-bit PCM, 48 kHz, stereo, cut into fixed blocks of
//! `BLOCK_FRAMES` frames. Every audio packet carries its own block plus a copy
//! of the previous block, so a single lost datagram is recovered from the next
//! one without waiting. Longer gaps are repaired with NACK + retransmit.
//!
//! Header (16 bytes, little endian):
//!   0..4   magic "AON1"
//!   4      kind
//!   5      codec of audio payloads (0 PCM 24-bit, 1 PCM 16-bit, 2 Opus)
//!   6..8   reserved
//!   8..12  stream id (random per sender session)
//!   12..16 sequence number of the first block in the packet
//!
//! Audio / Retransmit payload, PCM: 1 or 2 blocks; when 2 are present the
//! second one is block `seq - 1` (the redundant copy).
//! Audio / Retransmit payload, Opus: 1 or 2 frames, each a u16 length then
//! the frame. A frame covers `OPUS_FRAME_BLOCKS` blocks starting at `seq`;
//! the second frame is the previous one (starting at `seq - OPUS_FRAME_BLOCKS`).
//! Nack payload: a list of u32 sequence numbers.
//! Hello payload (sender -> hub, once a second): device id u64, source u8
//! (0 system audio, 1 mic), os u8 (0 Windows, 1 macOS), then the UTF-8
//! device name. It names the stream and doubles as a keepalive.
//! Status payload (hub -> sender, once a second): buffer depth in 0.1 ms, u16.
//! Want payload (client -> host, once a second while wanted): device id u64,
//! return port u16, flags u8 (bit 0 host system audio, bit 1 host mic). Asks
//! the host to stream back to the client's address on that port.

pub const MAGIC: [u8; 4] = *b"AON1";
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// 96 frames = 2 ms. Two blocks plus header still fit in one 1500-byte MTU.
pub const BLOCK_FRAMES: usize = 96;
pub const BLOCK_SAMPLES: usize = BLOCK_FRAMES * CHANNELS;
pub const BLOCK_BYTES: usize = BLOCK_SAMPLES * 3;
pub const HEADER_BYTES: usize = 16;
pub const MAX_PACKET_BYTES: usize = HEADER_BYTES + 2 * BLOCK_BYTES;
pub const DEFAULT_PORT: u16 = 47_100;

pub type Block = [f32; BLOCK_SAMPLES];

/// One Opus frame is 10 ms: five 2 ms blocks.
pub const OPUS_FRAME_BLOCKS: usize = 5;
pub const OPUS_FRAME_FRAMES: usize = OPUS_FRAME_BLOCKS * BLOCK_FRAMES;

/// What a sender puts on the wire, picked by the user per device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Quality {
    /// 24-bit PCM, uncompressed.
    #[default]
    Highest,
    /// 16-bit PCM.
    Cd,
    /// Opus at about 256 kbps.
    Balanced,
    /// Opus at about 96 kbps.
    Saver,
}

impl Quality {
    pub fn codec(self) -> Codec {
        match self {
            Quality::Highest => Codec::Pcm24,
            Quality::Cd => Codec::Pcm16,
            Quality::Balanced | Quality::Saver => Codec::Opus,
        }
    }

    /// Opus bitrate in bits per second; `None` for PCM.
    pub fn opus_bitrate(self) -> Option<i32> {
        match self {
            Quality::Balanced => Some(256_000),
            Quality::Saver => Some(96_000),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Quality::Highest => "highest",
            Quality::Cd => "cd",
            Quality::Balanced => "balanced",
            Quality::Saver => "saver",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            // "lossless" is what 0.1.x saved.
            "highest" | "lossless" => Quality::Highest,
            "cd" => Quality::Cd,
            "balanced" => Quality::Balanced,
            "saver" => Quality::Saver,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Codec {
    Pcm24 = 0,
    Pcm16 = 1,
    Opus = 2,
}

impl Codec {
    /// Bytes per sample for PCM codecs.
    fn width(self) -> usize {
        match self {
            Codec::Pcm16 => 2,
            _ => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Audio = 1,
    Retransmit = 2,
    Nack = 3,
    Hello = 4,
    Status = 5,
    Want = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    System,
    Mic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
}

#[derive(Debug, PartialEq)]
pub enum Packet {
    /// PCM. `blocks[0]` is `seq`, `blocks[1]` (if any) is `seq - 1`.
    Audio { stream: u32, seq: u32, blocks: Vec<Box<Block>>, retransmit: bool },
    /// Opus. `frames[0]` starts at `seq`, `frames[1]` (if any) at
    /// `seq - OPUS_FRAME_BLOCKS`. Decoding is the receiver's job (it needs
    /// a stateful decoder per stream).
    Opus { stream: u32, seq: u32, frames: Vec<Vec<u8>>, retransmit: bool },
    Nack { stream: u32, seqs: Vec<u32> },
    Hello { stream: u32, device: u64, source: Source, os: Os, name: String },
    Status { stream: u32, buffer_ms: f32 },
    Want { device: u64, port: u16, system: bool, mic: bool },
}

fn header(buf: &mut Vec<u8>, kind: Kind, stream: u32, seq: u32) {
    header_codec(buf, kind, Codec::Pcm24, stream, seq);
}

fn header_codec(buf: &mut Vec<u8>, kind: Kind, codec: Codec, stream: u32, seq: u32) {
    buf.clear();
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&[kind as u8, codec as u8, 0, 0]);
    buf.extend_from_slice(&stream.to_le_bytes());
    buf.extend_from_slice(&seq.to_le_bytes());
}

fn to_i24(s: f32) -> [u8; 3] {
    let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
    let b = v.to_le_bytes();
    [b[0], b[1], b[2]]
}

fn to_i16(s: f32) -> [u8; 2] {
    ((s.clamp(-1.0, 1.0) * 32_767.0).round() as i16).to_le_bytes()
}

fn from_i16(b: &[u8]) -> f32 {
    i16::from_le_bytes([b[0], b[1]]) as f32 / 32_767.0
}

fn from_i24(b: &[u8]) -> f32 {
    // Place the 3 bytes in the top of an i32 and shift back to sign-extend.
    let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
    v as f32 / 8_388_607.0
}

/// Encode a PCM audio packet (`codec` is Pcm24 or Pcm16). `prev` is block
/// `seq - 1`, sent as the redundant copy.
pub fn encode_audio(buf: &mut Vec<u8>, codec: Codec, stream: u32, seq: u32, block: &Block, prev: Option<&Block>, retransmit: bool) {
    let kind = if retransmit { Kind::Retransmit } else { Kind::Audio };
    header_codec(buf, kind, codec, stream, seq);
    for b in std::iter::once(block).chain(prev) {
        for &s in b.iter() {
            match codec {
                Codec::Pcm16 => buf.extend_from_slice(&to_i16(s)),
                _ => buf.extend_from_slice(&to_i24(s)),
            }
        }
    }
}

/// Encode an Opus packet: `frame` starts at block `seq`, `prev` is the
/// frame before it (the redundant copy).
pub fn encode_opus(buf: &mut Vec<u8>, stream: u32, seq: u32, frame: &[u8], prev: Option<&[u8]>, retransmit: bool) {
    let kind = if retransmit { Kind::Retransmit } else { Kind::Audio };
    header_codec(buf, kind, Codec::Opus, stream, seq);
    for f in std::iter::once(frame).chain(prev) {
        buf.extend_from_slice(&(f.len() as u16).to_le_bytes());
        buf.extend_from_slice(f);
    }
}

pub fn encode_nack(buf: &mut Vec<u8>, stream: u32, seqs: &[u32]) {
    header(buf, Kind::Nack, stream, 0);
    for s in seqs {
        buf.extend_from_slice(&s.to_le_bytes());
    }
}

pub fn encode_hello(buf: &mut Vec<u8>, stream: u32, device: u64, source: Source, os: Os, name: &str) {
    header(buf, Kind::Hello, stream, 0);
    buf.extend_from_slice(&device.to_le_bytes());
    buf.push(source as u8);
    buf.push(os as u8);
    let mut end = name.len().min(64);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    buf.extend_from_slice(&name.as_bytes()[..end]);
}

pub fn encode_status(buf: &mut Vec<u8>, stream: u32, buffer_ms: f32) {
    header(buf, Kind::Status, stream, 0);
    buf.extend_from_slice(&((buffer_ms * 10.0).clamp(0.0, 65535.0) as u16).to_le_bytes());
}

pub fn encode_want(buf: &mut Vec<u8>, device: u64, port: u16, system: bool, mic: bool) {
    header(buf, Kind::Want, 0, 0);
    buf.extend_from_slice(&device.to_le_bytes());
    buf.extend_from_slice(&port.to_le_bytes());
    buf.push(system as u8 | (mic as u8) << 1);
}

pub fn decode(data: &[u8]) -> Option<Packet> {
    if data.len() < HEADER_BYTES || data[0..4] != MAGIC {
        return None;
    }
    let stream = u32::from_le_bytes(data[8..12].try_into().unwrap());
    let seq = u32::from_le_bytes(data[12..16].try_into().unwrap());
    let payload = &data[HEADER_BYTES..];
    match data[4] {
        k @ (1 | 2) if data[5] == Codec::Opus as u8 => {
            let mut frames = Vec::new();
            let mut rest = payload;
            while rest.len() >= 2 && frames.len() < 2 {
                let n = u16::from_le_bytes([rest[0], rest[1]]) as usize;
                let f = rest.get(2..2 + n)?;
                frames.push(f.to_vec());
                rest = &rest[2 + n..];
            }
            if frames.is_empty() {
                return None;
            }
            Some(Packet::Opus { stream, seq, frames, retransmit: k == 2 })
        }
        k @ (1 | 2) => {
            let codec = if data[5] == Codec::Pcm16 as u8 { Codec::Pcm16 } else { Codec::Pcm24 };
            let block_bytes = BLOCK_SAMPLES * codec.width();
            let n = payload.len() / block_bytes;
            if n == 0 || n > 2 || payload.len() % block_bytes != 0 {
                return None;
            }
            let blocks = payload
                .chunks_exact(block_bytes)
                .map(|chunk| {
                    let mut b = Box::new([0.0; BLOCK_SAMPLES]);
                    for (s, bytes) in b.iter_mut().zip(chunk.chunks_exact(codec.width())) {
                        *s = if codec == Codec::Pcm16 { from_i16(bytes) } else { from_i24(bytes) };
                    }
                    b
                })
                .collect();
            Some(Packet::Audio { stream, seq, blocks, retransmit: k == 2 })
        }
        3 => {
            if payload.len() % 4 != 0 {
                return None;
            }
            let seqs = payload.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            Some(Packet::Nack { stream, seqs })
        }
        4 => {
            if payload.len() < 10 {
                return None;
            }
            let device = u64::from_le_bytes(payload[0..8].try_into().unwrap());
            let source = if payload[8] == 1 { Source::Mic } else { Source::System };
            let os = if payload[9] == 1 { Os::MacOs } else { Os::Windows };
            let name = String::from_utf8_lossy(&payload[10..]).into_owned();
            Some(Packet::Hello { stream, device, source, os, name })
        }
        5 => {
            let raw = u16::from_le_bytes(payload.get(0..2)?.try_into().unwrap());
            Some(Packet::Status { stream, buffer_ms: raw as f32 / 10.0 })
        }
        6 => {
            let p = payload.get(0..11)?;
            let device = u64::from_le_bytes(p[0..8].try_into().unwrap());
            let port = u16::from_le_bytes(p[8..10].try_into().unwrap());
            Some(Packet::Want { device, port, system: p[10] & 1 != 0, mic: p[10] & 2 != 0 })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(offset: f32) -> Block {
        std::array::from_fn(|i| ((i as f32 / BLOCK_SAMPLES as f32) * 2.0 - 1.0) * 0.9 + offset * 0.01)
    }

    #[test]
    fn audio_roundtrip_is_24bit_exact() {
        let a = ramp(0.0);
        let b = ramp(1.0);
        let mut buf = Vec::new();
        encode_audio(&mut buf, Codec::Pcm24, 7, 42, &a, Some(&b), false);
        assert_eq!(buf.len(), MAX_PACKET_BYTES);
        assert!(MAX_PACKET_BYTES <= 1472, "must fit a single 1500 MTU datagram");
        let Some(Packet::Audio { stream, seq, blocks, retransmit }) = decode(&buf) else { panic!() };
        assert_eq!((stream, seq, blocks.len(), retransmit), (7, 42, 2, false));
        for (orig, got) in a.iter().chain(b.iter()).zip(blocks.iter().flat_map(|b| b.iter())) {
            assert!((orig - got).abs() <= 0.5 / 8_388_607.0 + f32::EPSILON);
        }
    }

    #[test]
    fn hello_and_status_roundtrip() {
        let mut buf = Vec::new();
        encode_hello(&mut buf, 3, 0xdead_beef_0102, Source::Mic, Os::MacOs, "office-mac");
        assert_eq!(
            decode(&buf),
            Some(Packet::Hello { stream: 3, device: 0xdead_beef_0102, source: Source::Mic, os: Os::MacOs, name: "office-mac".into() })
        );
        encode_status(&mut buf, 3, 21.8);
        assert_eq!(decode(&buf), Some(Packet::Status { stream: 3, buffer_ms: 21.8 }));
        encode_want(&mut buf, 77, 50_123, false, true);
        assert_eq!(decode(&buf), Some(Packet::Want { device: 77, port: 50_123, system: false, mic: true }));
    }

    #[test]
    fn pcm16_roundtrip_and_opus_framing() {
        let a = ramp(0.0);
        let mut buf = Vec::new();
        encode_audio(&mut buf, Codec::Pcm16, 1, 5, &a, None, false);
        assert_eq!(buf.len(), HEADER_BYTES + BLOCK_SAMPLES * 2);
        let Some(Packet::Audio { blocks, .. }) = decode(&buf) else { panic!() };
        for (o, g) in a.iter().zip(blocks[0].iter()) {
            assert!((o - g).abs() <= 1.0 / 32_767.0);
        }
        encode_opus(&mut buf, 1, 10, &[1, 2, 3], Some(&[9, 8]), true);
        assert_eq!(
            decode(&buf),
            Some(Packet::Opus { stream: 1, seq: 10, frames: vec![vec![1, 2, 3], vec![9, 8]], retransmit: true })
        );
        for q in [Quality::Highest, Quality::Cd, Quality::Balanced, Quality::Saver] {
            assert_eq!(Quality::from_name(q.name()), Some(q));
        }
    }

    #[test]
    fn i24_extremes_and_sign() {
        for v in [-1.0f32, -0.5, 0.0, 0.25, 1.0] {
            assert!((from_i24(&to_i24(v)) - v).abs() < 1e-6);
        }
    }

    #[test]
    fn nack_roundtrip_and_garbage() {
        let mut buf = Vec::new();
        encode_nack(&mut buf, 9, &[1, 2, u32::MAX]);
        assert_eq!(decode(&buf), Some(Packet::Nack { stream: 9, seqs: vec![1, 2, u32::MAX] }));
        assert_eq!(decode(b"hello"), None);
        buf[0] = b'X';
        assert_eq!(decode(&buf), None);
    }
}
