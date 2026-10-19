//! ARP for IPv4 over Ethernet (RFC 826) and the neighbour cache.
//!
//! The cache maps addresses to MAC addresses for [`TTL`]. A lookup that
//! misses starts a resolution: the stack sends a request and parks the
//! packets for that address here; the reply releases them. Requests are
//! repeated every [`RETRY`] up to [`TRIES`] times, then the parked packets
//! are dropped (TCP retransmits; UDP is unreliable anyway).

use alloc::vec::Vec;

use crate::wire::{be16, ip_at};
use crate::{Ipv4, Mac};

pub const LEN: usize = 28;
pub const OP_REQUEST: u16 = 1;
pub const OP_REPLY: u16 = 2;

/// How long an entry is trusted.
pub const TTL: u64 = 10 * 60 * 1000;
/// Interval between requests for an unresolved address.
pub const RETRY: u64 = 1000;
pub const TRIES: u32 = 3;
/// Packets parked per unresolved address.
const MAX_PARKED: usize = 16;
const MAX_ENTRIES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet {
    pub op: u16,
    pub sender_mac: Mac,
    pub sender_ip: Ipv4,
    pub target_mac: Mac,
    pub target_ip: Ipv4,
}

pub fn parse(b: &[u8]) -> Option<Packet> {
    if b.len() < LEN || be16(b, 0) != 1 || be16(b, 2) != 0x0800 || b[4] != 6 || b[5] != 4 {
        return None;
    }
    let mut sm = [0; 6];
    let mut tm = [0; 6];
    sm.copy_from_slice(&b[8..14]);
    tm.copy_from_slice(&b[18..24]);
    Some(Packet { op: be16(b, 6), sender_mac: sm, sender_ip: ip_at(b, 14), target_mac: tm, target_ip: ip_at(b, 24) })
}

pub fn build(p: &Packet) -> [u8; LEN] {
    let mut b = [0u8; LEN];
    b[0..2].copy_from_slice(&1u16.to_be_bytes());
    b[2..4].copy_from_slice(&0x0800u16.to_be_bytes());
    b[4] = 6;
    b[5] = 4;
    b[6..8].copy_from_slice(&p.op.to_be_bytes());
    b[8..14].copy_from_slice(&p.sender_mac);
    b[14..18].copy_from_slice(&p.sender_ip.0);
    b[18..24].copy_from_slice(&p.target_mac);
    b[24..28].copy_from_slice(&p.target_ip.0);
    b
}

struct Entry {
    iface: usize,
    ip: Ipv4,
    /// None while being resolved.
    mac: Option<Mac>,
    expires: u64,
    /// Resolution: next request time and requests sent.
    retry_at: u64,
    tries: u32,
    /// IP packets waiting for the address.
    parked: Vec<Vec<u8>>,
}

/// What the cache wants the stack to do after a timer run.
pub enum Action {
    /// Send a request for this address on this interface.
    Request(usize, Ipv4),
}

#[derive(Default)]
pub struct Cache {
    entries: Vec<Entry>,
}

impl Cache {
    pub fn lookup(&self, iface: usize, ip: Ipv4, now: u64) -> Option<Mac> {
        self.entries.iter().find(|e| e.iface == iface && e.ip == ip && e.expires > now).and_then(|e| e.mac)
    }

    /// Record a mapping. With `create` false only an existing entry is
    /// refreshed (RFC 826's merge flag). Returns the packets that waited
    /// for it.
    pub fn update(&mut self, iface: usize, ip: Ipv4, mac: Mac, now: u64, create: bool) -> Vec<Vec<u8>> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.iface == iface && e.ip == ip) {
            e.mac = Some(mac);
            e.expires = now + TTL;
            return core::mem::take(&mut e.parked);
        }
        if create {
            if self.entries.len() >= MAX_ENTRIES {
                // Evict the entry closest to expiry.
                if let Some(i) = (0..self.entries.len()).min_by_key(|&i| self.entries[i].expires) {
                    self.entries.swap_remove(i);
                }
            }
            self.entries.push(Entry { iface, ip, mac: Some(mac), expires: now + TTL, retry_at: 0, tries: 0, parked: Vec::new() });
        }
        Vec::new()
    }

    /// Park `packet` until `ip` is resolved. Returns true when a request
    /// should go out now (a new resolution).
    pub fn park(&mut self, iface: usize, ip: Ipv4, packet: Vec<u8>, now: u64) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.iface == iface && e.ip == ip) {
            if e.mac.is_some() && e.expires > now {
                // Raced with a reply: the caller looks up again.
                e.parked.push(packet);
                return false;
            }
            if e.mac.is_some() {
                // Expired: resolve again.
                e.mac = None;
                e.tries = 0;
            }
            if e.parked.len() < MAX_PARKED {
                e.parked.push(packet);
            }
            if e.tries == 0 {
                e.tries = 1;
                e.retry_at = now + RETRY;
                e.expires = now + RETRY * TRIES as u64 + 1;
                return true;
            }
            return false;
        }
        self.entries.push(Entry {
            iface,
            ip,
            mac: None,
            expires: now + RETRY * TRIES as u64 + 1,
            retry_at: now + RETRY,
            tries: 1,
            parked: alloc::vec![packet],
        });
        true
    }

    /// Take the packets parked for an address that is resolved now.
    pub fn take_parked(&mut self, iface: usize, ip: Ipv4) -> Vec<Vec<u8>> {
        match self.entries.iter_mut().find(|e| e.iface == iface && e.ip == ip && e.mac.is_some()) {
            Some(e) => core::mem::take(&mut e.parked),
            None => Vec::new(),
        }
    }

    /// Timers: repeat requests, give up on silent addresses, expire
    /// entries.
    pub fn poll(&mut self, now: u64, out: &mut Vec<Action>) {
        self.entries.retain_mut(|e| {
            if e.mac.is_none() {
                if now >= e.retry_at {
                    if e.tries >= TRIES {
                        return false;
                    }
                    e.tries += 1;
                    e.retry_at = now + RETRY;
                    out.push(Action::Request(e.iface, e.ip));
                }
                return true;
            }
            e.expires > now
        });
    }

    /// Forget everything learnt on an interface (address change).
    pub fn flush(&mut self, iface: usize) {
        self.entries.retain(|e| e.iface != iface);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_roundtrip() {
        let p = Packet {
            op: OP_REQUEST,
            sender_mac: [0x52, 0x54, 0, 0x12, 0x34, 0x56],
            sender_ip: Ipv4::new(10, 0, 2, 15),
            target_mac: [0; 6],
            target_ip: Ipv4::new(10, 0, 2, 2),
        };
        assert_eq!(parse(&build(&p)), Some(p));
    }

    #[test]
    fn resolution() {
        let mut c = Cache::default();
        let ip = Ipv4::new(10, 0, 2, 2);
        assert!(c.park(0, ip, alloc::vec![1], 0));
        assert!(!c.park(0, ip, alloc::vec![2], 10));
        let mut acts = Vec::new();
        c.poll(500, &mut acts);
        assert!(acts.is_empty());
        c.poll(1000, &mut acts);
        assert_eq!(acts.len(), 1);
        let parked = c.update(0, ip, [7; 6], 1100, false);
        assert_eq!(parked, alloc::vec![alloc::vec![1], alloc::vec![2]]);
        assert_eq!(c.lookup(0, ip, 1200), Some([7; 6]));
        assert_eq!(c.lookup(0, ip, 1100 + TTL), None);
    }

    #[test]
    fn gives_up() {
        let mut c = Cache::default();
        let ip = Ipv4::new(10, 0, 2, 9);
        c.park(0, ip, alloc::vec![1], 0);
        let mut acts = Vec::new();
        for t in (0..10).map(|i| i * 1000) {
            c.poll(t, &mut acts);
        }
        assert_eq!(acts.len(), 2);
        assert!(c.is_empty());
    }
}
