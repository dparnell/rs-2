//! UDP (RFC 768).

use alloc::vec::Vec;

use crate::wire::{be16, put16, transport_checksum};
use crate::ipv4::PROTO_UDP;
use crate::Ipv4;

pub const HEADER: usize = 8;

pub struct Datagram<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub data: &'a [u8],
}

/// Parse a datagram carried from `src` to `dst`, checking the checksum
/// when the sender computed one.
pub fn parse(src: Ipv4, dst: Ipv4, b: &[u8]) -> Option<Datagram<'_>> {
    if b.len() < HEADER {
        return None;
    }
    let len = be16(b, 4) as usize;
    if len < HEADER || len > b.len() {
        return None;
    }
    if be16(b, 6) != 0 && transport_checksum(src, dst, PROTO_UDP, &b[..len]) != 0 {
        return None;
    }
    Some(Datagram { src_port: be16(b, 0), dst_port: be16(b, 2), data: &b[HEADER..len] })
}

pub fn build(src: Ipv4, sport: u16, dst: Ipv4, dport: u16, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER + data.len());
    v.extend_from_slice(&sport.to_be_bytes());
    v.extend_from_slice(&dport.to_be_bytes());
    v.extend_from_slice(&((HEADER + data.len()) as u16).to_be_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(data);
    let mut c = transport_checksum(src, dst, PROTO_UDP, &v);
    if c == 0 {
        c = 0xffff;
    }
    put16(&mut v, 6, c);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let (a, b) = (Ipv4::new(10, 0, 2, 15), Ipv4::new(10, 0, 2, 3));
        let d = build(a, 1234, b, 53, b"query");
        let p = parse(a, b, &d).unwrap();
        assert_eq!((p.src_port, p.dst_port, p.data), (1234, 53, &b"query"[..]));
        // Wrong pseudo-header: checksum fails.
        assert!(parse(a, Ipv4::new(10, 0, 2, 4), &d).is_none());
    }
}
