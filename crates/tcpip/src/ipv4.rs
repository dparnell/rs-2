//! IPv4 (RFC 791): the header, fragmentation of outgoing datagrams and
//! reassembly of incoming ones (RFC 815's hole list, kept as a list of
//! received ranges).

use alloc::vec::Vec;

use crate::wire::{be16, checksum, ip_at, put16};
use crate::Ipv4;

pub const HEADER: usize = 20;
pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;
pub const DEFAULT_TTL: u8 = 64;

const FLAG_DF: u16 = 0x4000;
const FLAG_MF: u16 = 0x2000;

/// How long fragments of an incomplete datagram are kept (RFC 791
/// suggests 15 s; most stacks use 30-60 s).
pub const REASSEMBLY_TIMEOUT: u64 = 30_000;
/// Datagrams being reassembled at once.
const MAX_REASSEMBLIES: usize = 16;
const MAX_DATAGRAM: usize = 65535;

#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub ihl: usize,
    pub total_len: usize,
    pub id: u16,
    pub dont_fragment: bool,
    pub more_fragments: bool,
    /// Fragment offset in bytes.
    pub frag_offset: usize,
    pub ttl: u8,
    pub proto: u8,
    pub src: Ipv4,
    pub dst: Ipv4,
}

/// Parse and validate the header of `b`: version, lengths and checksum.
pub fn parse(b: &[u8]) -> Option<Header> {
    if b.len() < HEADER || b[0] >> 4 != 4 {
        return None;
    }
    let ihl = (b[0] & 0xf) as usize * 4;
    let total_len = be16(b, 2) as usize;
    if ihl < HEADER || total_len < ihl || total_len > b.len() || checksum(&b[..ihl]) != 0 {
        return None;
    }
    let ff = be16(b, 6);
    Some(Header {
        ihl,
        total_len,
        id: be16(b, 4),
        dont_fragment: ff & FLAG_DF != 0,
        more_fragments: ff & FLAG_MF != 0,
        frag_offset: (ff & 0x1fff) as usize * 8,
        ttl: b[8],
        proto: b[9],
        src: ip_at(b, 12),
        dst: ip_at(b, 16),
    })
}

/// A 20-byte header (no options) with its checksum.
pub fn header(src: Ipv4, dst: Ipv4, proto: u8, id: u16, payload_len: usize, frag: u16, ttl: u8) -> [u8; HEADER] {
    let mut h = [0u8; HEADER];
    h[0] = 0x45;
    put16(&mut h, 2, (HEADER + payload_len) as u16);
    put16(&mut h, 4, id);
    put16(&mut h, 6, frag);
    h[8] = ttl;
    h[9] = proto;
    h[12..16].copy_from_slice(&src.0);
    h[16..20].copy_from_slice(&dst.0);
    let c = checksum(&h);
    put16(&mut h, 10, c);
    h
}

/// A complete datagram.
pub fn build(src: Ipv4, dst: Ipv4, proto: u8, id: u16, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER + payload.len());
    v.extend_from_slice(&header(src, dst, proto, id, payload.len(), FLAG_DF, DEFAULT_TTL));
    v.extend_from_slice(payload);
    v
}

/// Split a datagram's payload into fragments that fit `mtu`.
pub fn fragment(src: Ipv4, dst: Ipv4, proto: u8, id: u16, payload: &[u8], mtu: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if HEADER + payload.len() <= mtu {
        out.push(build(src, dst, proto, id, payload));
        return out;
    }
    let chunk = ((mtu - HEADER) / 8 * 8).max(8);
    let mut off = 0;
    while off < payload.len() {
        let end = (off + chunk).min(payload.len());
        let mf = if end < payload.len() { FLAG_MF } else { 0 };
        let mut v = Vec::with_capacity(HEADER + end - off);
        v.extend_from_slice(&header(src, dst, proto, id, end - off, mf | (off / 8) as u16, DEFAULT_TTL));
        v.extend_from_slice(&payload[off..end]);
        out.push(v);
        off = end;
    }
    out
}

struct Partial {
    src: Ipv4,
    dst: Ipv4,
    proto: u8,
    id: u16,
    data: Vec<u8>,
    /// Received byte ranges [start, end), sorted and merged.
    have: Vec<(usize, usize)>,
    /// Payload length, once the last fragment arrived.
    total: Option<usize>,
    expires: u64,
}

impl Partial {
    fn add(&mut self, off: usize, data: &[u8]) {
        let end = off + data.len();
        if self.data.len() < end {
            self.data.resize(end, 0);
        }
        self.data[off..end].copy_from_slice(data);
        self.have.push((off, end));
        self.have.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for &(s, e) in &self.have {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        self.have = merged;
    }

    fn complete(&self) -> bool {
        self.total.is_some_and(|t| self.have.len() == 1 && self.have[0] == (0, t))
    }
}

/// Fragments waiting for the rest of their datagram.
#[derive(Default)]
pub struct Reassembler {
    partial: Vec<Partial>,
}

impl Reassembler {
    /// Add a fragment (`payload` is the data after the header). Returns the
    /// whole payload once every fragment is in.
    pub fn add(&mut self, h: &Header, payload: &[u8], now: u64) -> Option<Vec<u8>> {
        let end = h.frag_offset + payload.len();
        if end > MAX_DATAGRAM - HEADER || (h.more_fragments && payload.len() % 8 != 0) {
            return None;
        }
        let i = match self.partial.iter().position(|p| p.src == h.src && p.dst == h.dst && p.proto == h.proto && p.id == h.id) {
            Some(i) => i,
            None => {
                if self.partial.len() >= MAX_REASSEMBLIES {
                    // Drop the oldest.
                    let oldest = (0..self.partial.len()).min_by_key(|&i| self.partial[i].expires).unwrap();
                    self.partial.swap_remove(oldest);
                }
                self.partial.push(Partial {
                    src: h.src,
                    dst: h.dst,
                    proto: h.proto,
                    id: h.id,
                    data: Vec::new(),
                    have: Vec::new(),
                    total: None,
                    expires: now + REASSEMBLY_TIMEOUT,
                });
                self.partial.len() - 1
            }
        };
        let p = &mut self.partial[i];
        if !h.more_fragments {
            if p.total.is_some_and(|t| t != end) {
                self.partial.swap_remove(i);
                return None;
            }
            p.total = Some(end);
        }
        if p.total.is_some_and(|t| end > t) {
            self.partial.swap_remove(i);
            return None;
        }
        p.add(h.frag_offset, payload);
        if p.complete() {
            let mut p = self.partial.swap_remove(i);
            p.data.truncate(p.total.unwrap());
            return Some(p.data);
        }
        None
    }

    /// Drop datagrams that did not complete in time.
    pub fn poll(&mut self, now: u64) {
        self.partial.retain(|p| p.expires > now);
    }

    pub fn pending(&self) -> usize {
        self.partial.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a() -> Ipv4 {
        Ipv4::new(10, 0, 2, 15)
    }
    fn b() -> Ipv4 {
        Ipv4::new(10, 0, 2, 2)
    }

    #[test]
    fn header_roundtrip() {
        let d = build(a(), b(), PROTO_UDP, 7, &[1, 2, 3]);
        let h = parse(&d).unwrap();
        assert_eq!((h.src, h.dst, h.proto, h.id, h.total_len), (a(), b(), PROTO_UDP, 7, 23));
        assert!(h.dont_fragment && !h.more_fragments);
        let mut bad = d.clone();
        bad[8] ^= 1;
        assert!(parse(&bad).is_none());
    }

    #[test]
    fn fragment_and_reassemble_out_of_order() {
        let payload: Vec<u8> = (0..4000u32).map(|i| i as u8).collect();
        let frags = fragment(a(), b(), PROTO_UDP, 99, &payload, 1500);
        assert_eq!(frags.len(), 3);
        let mut r = Reassembler::default();
        let mut got = None;
        for f in [&frags[2], &frags[0], &frags[0], &frags[1]] {
            let h = parse(f).unwrap();
            assert!(f.len() <= 1500);
            got = r.add(&h, &f[h.ihl..h.total_len], 0);
        }
        assert_eq!(got.unwrap(), payload);
        assert_eq!(r.pending(), 0);
    }

    #[test]
    fn reassembly_times_out() {
        let payload = [0u8; 3000];
        let frags = fragment(a(), b(), PROTO_UDP, 5, &payload, 1500);
        let mut r = Reassembler::default();
        let h = parse(&frags[0]).unwrap();
        assert!(r.add(&h, &frags[0][h.ihl..], 0).is_none());
        r.poll(REASSEMBLY_TIMEOUT + 1);
        assert_eq!(r.pending(), 0);
    }
}
