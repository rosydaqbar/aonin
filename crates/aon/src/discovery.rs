//! Zero-config discovery: every running Aonin broadcasts a small JSON beacon
//! on the local network once a second and listens for the others'. Nothing
//! leaves the LAN and there is no server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const BEACON_PORT: u16 = 47_101;
const MAGIC: &[u8] = b"AONB";
const EVERY: Duration = Duration::from_secs(1);
/// A peer that misses this many beacons is gone.
const EXPIRE: Duration = Duration::from_secs(4);

/// What a device announces about itself.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Advert {
    pub id: u64,
    pub name: String,
    pub macos: bool,
    pub hosting: bool,
    /// UDP port the hub listens on.
    pub port: u16,
    pub clients: u32,
    /// Every device whose audio reaches this one, directly or through other
    /// hosts. A device must never join a host listed in its own sources.
    pub sources: Vec<u64>,
}

#[derive(Clone, Debug)]
pub struct Peer {
    pub advert: Advert,
    pub ip: IpAddr,
    pub last_seen: Instant,
}

#[derive(Default)]
pub struct Discovery {
    pub stop: AtomicBool,
    pub me: Mutex<Advert>,
    pub peers: Mutex<HashMap<u64, Peer>>,
    /// Set when the beacon socket could not be opened (port taken, firewall).
    pub error: Mutex<Option<String>>,
}

impl Discovery {
    pub fn peers(&self) -> Vec<Peer> {
        let mut v: Vec<Peer> = self.peers.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.advert.name.to_lowercase().cmp(&b.advert.name.to_lowercase()));
        v
    }

    /// Broadcast and listen until `stop` is set.
    pub fn run(&self) {
        let sock = match bind() {
            Ok(s) => s,
            Err(e) => {
                *self.error.lock().unwrap() = Some(e.to_string());
                return;
            }
        };
        let mut buf = [0u8; 4096];
        let mut last_sent = Instant::now() - EVERY;
        let mut last_advert = Advert::default();
        while !self.stop.load(Relaxed) {
            let me = self.me.lock().unwrap().clone();
            // Announce on a timer, and at once when something changed.
            if last_sent.elapsed() >= EVERY || me != last_advert {
                let mut msg = MAGIC.to_vec();
                msg.extend(serde_json::to_vec(&me).unwrap_or_default());
                for to in broadcast_targets() {
                    let _ = sock.send_to(&msg, SocketAddr::new(to.into(), BEACON_PORT));
                }
                last_sent = Instant::now();
                last_advert = me.clone();
            }
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                if let Some(ad) = buf[..n].strip_prefix(MAGIC).and_then(|j| serde_json::from_slice::<Advert>(j).ok()) {
                    if ad.id != me.id {
                        self.peers.lock().unwrap().insert(ad.id, Peer { advert: ad, ip: from.ip(), last_seen: Instant::now() });
                    }
                }
            }
            self.peers.lock().unwrap().retain(|_, p| p.last_seen.elapsed() < EXPIRE);
        }
    }
}

fn bind() -> std::io::Result<UdpSocket> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, BEACON_PORT))?;
    sock.set_broadcast(true)?;
    sock.set_read_timeout(Some(Duration::from_millis(200)))?;
    Ok(sock)
}

/// The limited broadcast address only leaves through one interface on most
/// systems, so also send to each interface's directed broadcast address.
fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut out = vec![Ipv4Addr::BROADCAST];
    if let Ok(ifs) = if_addrs::get_if_addrs() {
        for i in ifs {
            if let if_addrs::IfAddr::V4(v4) = i.addr {
                if let Some(b) = v4.broadcast.filter(|_| !v4.ip.is_loopback()) {
                    if !out.contains(&b) {
                        out.push(b);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advert_roundtrips_as_json() {
        let a = Advert { id: 42, name: "office-mac".into(), macos: true, hosting: true, port: 47_100, clients: 2, sources: vec![7, 9] };
        let j = serde_json::to_vec(&a).unwrap();
        assert_eq!(serde_json::from_slice::<Advert>(&j).unwrap(), a);
    }
}
