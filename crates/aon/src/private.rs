//! Private sessions: a listener shows a 6-character code, and an audio
//! sender that types it gets a key shared with that listener only.
//!
//! The code never travels. Both sides run SPAKE2 with it: the sender sends
//! `PairStart`, the listener answers `PairReply` with its half and its name
//! sealed under the resulting key. A wrong code gives the two sides different
//! keys, so the sender can't open the name and the listener can't open
//! anything the sender seals. Every later packet between them is sealed with
//! XChaCha20-Poly1305 (random 24-byte nonces, so any number of sockets can
//! share the key).
//!
//! Sealed packet: "AONS", nonce (24), then the encrypted plain packet with
//! its 16-byte tag.
//! PairStart:  "AONP", 1, SPAKE2 message.
//! PairReply:  "AONP", 2, u8 length, SPAKE2 message, sealed listener name.
//!
//! This file is the audio sender's side, which every edition has. The
//! listener's side is Aonin Pro (see `pro`).

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use x25519_dalek::{PublicKey, StaticSecret};

const SEALED: &[u8; 4] = b"AONS";
pub const PAIR: &[u8; 4] = b"AONP";
pub const UPGRADE: &[u8; 4] = b"AONU";
const NONCE: usize = 24;
/// Bytes a sealed packet adds: magic, nonce, tag.
pub const OVERHEAD: usize = 4 + NONCE + 16;

/// No 0/O or 1/I: the code is read off one screen and typed on another.
pub const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const CODE_LEN: usize = 6;

/// SPAKE2 identities of the two sides.
pub const SENDER_ID: &[u8] = b"aonin audio sender";
pub const LISTENER_ID: &[u8] = b"aonin listener";

pub fn new_code() -> String {
    let mut bytes = [0u8; CODE_LEN];
    getrandom::getrandom(&mut bytes).expect("system random");
    bytes
        .iter()
        .map(|b| CODE_ALPHABET[(*b as usize) % CODE_ALPHABET.len()] as char)
        .collect()
}

/// The code as typed ("k7q 4mz", "K7Q-4MZ") in its canonical form, or
/// `None` if it can't be a code.
pub fn normalize_code(typed: &str) -> Option<String> {
    let code: String = typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (code.len() == CODE_LEN && code.bytes().all(|b| CODE_ALPHABET.contains(&b))).then_some(code)
}

pub struct Cipher(XChaCha20Poly1305);

impl Cipher {
    pub fn new(key: &[u8]) -> Self {
        // SPAKE2 yields 32 bytes, exactly a key.
        Self(XChaCha20Poly1305::new_from_slice(&key[..32]).expect("32-byte key"))
    }

    /// Seal `plain` into `out` (replacing what was there).
    pub fn seal(&self, plain: &[u8], out: &mut Vec<u8>) {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let body = self.0.encrypt(&nonce, plain).expect("encrypt");
        out.clear();
        out.extend_from_slice(SEALED);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&body);
    }

    /// The plain packet, or `None` if it isn't sealed with this key.
    pub fn open(&self, data: &[u8]) -> Option<Vec<u8>> {
        if !is_sealed(data) || data.len() < OVERHEAD {
            return None;
        }
        let nonce = XNonce::from_slice(&data[4..4 + NONCE]);
        self.0.decrypt(nonce, &data[4 + NONCE..]).ok()
    }
}

pub fn is_sealed(data: &[u8]) -> bool {
    data.starts_with(SEALED)
}

/// Sender side of one pairing attempt with one listener.
pub struct Pairing {
    state: Option<Spake2<Ed25519Group>>,
    /// The PairStart packet, resent until a reply arrives.
    pub start: Vec<u8>,
}

impl Pairing {
    pub fn new(code: &str) -> Self {
        let (state, msg) = Spake2::<Ed25519Group>::start_a(
            &Password::new(code.as_bytes()),
            &Identity::new(SENDER_ID),
            &Identity::new(LISTENER_ID),
        );
        let mut start = PAIR.to_vec();
        start.push(1);
        start.extend_from_slice(&msg);
        Self {
            state: Some(state),
            start,
        }
    }

    /// The listener answered (rightly or not); nothing more to send.
    pub fn answered(&self) -> bool {
        self.state.is_none()
    }

    /// Finish with the listener's reply: its key and name, or `None` when
    /// the code was wrong (or the reply isn't one).
    pub fn finish(&mut self, reply: &[u8]) -> Option<(Cipher, String)> {
        let rest = reply.strip_prefix(PAIR)?.strip_prefix(&[2])?;
        let (&n, rest) = rest.split_first()?;
        let (msg, sealed) = (rest.get(..n as usize)?, rest.get(n as usize..)?);
        let key = self.state.take()?.finish(msg).ok()?;
        let cipher = Cipher::new(&key);
        let name = String::from_utf8(cipher.open(sealed)?).ok()?;
        Some((cipher, name))
    }
}

// The listener side (answering PairStart) is Aonin Pro: see `pro::PrivateHubOps`.

/// An audio sender already connected when its listener turned private
/// asks for the code (`UpgradeRequest`: "AONU", 1, X25519 public key). The
/// listener answers only devices that were connected at that moment
/// (`UpgradeReply`: "AONU", 2, its public key, the code sealed under the
/// shared key), and the sender then joins with the code as usual. Someone
/// only listening on the network learns nothing.
pub struct Upgrade {
    secret: StaticSecret,
    pub request: Vec<u8>,
}

impl Default for Upgrade {
    fn default() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let mut request = UPGRADE.to_vec();
        request.push(1);
        request.extend_from_slice(PublicKey::from(&secret).as_bytes());
        Self { secret, request }
    }
}

impl Upgrade {
    /// The code from the listener's reply.
    pub fn finish(&self, reply: &[u8]) -> Option<String> {
        let rest = reply.strip_prefix(UPGRADE)?.strip_prefix(&[2])?;
        let theirs: [u8; 32] = rest.get(..32)?.try_into().ok()?;
        let shared = self.secret.diffie_hellman(&PublicKey::from(theirs));
        let cipher = Cipher::new(&Sha256::digest(shared.as_bytes()));
        normalize_code(std::str::from_utf8(&cipher.open(&rest[32..])?).ok()?)
    }
}

// The listener side of `Upgrade` is Aonin Pro too.

pub fn is_upgrade_request(data: &[u8]) -> bool {
    data.len() > 5 && data.starts_with(UPGRADE) && data[4] == 1
}

pub fn is_upgrade_reply(data: &[u8]) -> bool {
    data.len() > 5 && data.starts_with(UPGRADE) && data[4] == 2
}

pub fn is_pair_start(data: &[u8]) -> bool {
    data.len() > 5 && data.starts_with(PAIR) && data[4] == 1
}

pub fn is_pair_reply(data: &[u8]) -> bool {
    data.len() > 5 && data.starts_with(PAIR) && data[4] == 2
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pairing and upgrade round trips need the listener side; they are
    // tested in aon-pro.

    #[test]
    fn codes() {
        for _ in 0..50 {
            let c = new_code();
            assert_eq!(normalize_code(&c), Some(c));
        }
        assert_eq!(normalize_code(" k7q-4mz "), Some("K7Q4MZ".into()));
        assert_eq!(normalize_code("K7Q4M"), None);
        assert_eq!(normalize_code("K7Q4M0"), None, "no zero in the alphabet");
    }
}
