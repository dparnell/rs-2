//! Ethernet II framing (RFC 894): destination, source, EtherType.

use alloc::vec::Vec;

use crate::wire::be16;
use crate::Mac;

pub const HEADER: usize = 14;
/// Largest payload of a standard frame.
pub const MTU: usize = 1500;
/// Shortest frame without the FCS; drivers pad (most NICs do it).
pub const MIN_FRAME: usize = 60;

pub const TYPE_IPV4: u16 = 0x0800;
pub const TYPE_ARP: u16 = 0x0806;

pub struct Frame<'a> {
    pub dst: Mac,
    pub src: Mac,
    pub ethertype: u16,
    pub payload: &'a [u8],
}

pub fn parse(b: &[u8]) -> Option<Frame<'_>> {
    if b.len() < HEADER {
        return None;
    }
    let mut dst = [0; 6];
    let mut src = [0; 6];
    dst.copy_from_slice(&b[0..6]);
    src.copy_from_slice(&b[6..12]);
    Some(Frame { dst, src, ethertype: be16(b, 12), payload: &b[HEADER..] })
}

/// A frame around `payload`, padded to the minimum frame length.
pub fn build(dst: Mac, src: Mac, ethertype: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity((HEADER + payload.len()).max(MIN_FRAME));
    f.extend_from_slice(&dst);
    f.extend_from_slice(&src);
    f.extend_from_slice(&ethertype.to_be_bytes());
    f.extend_from_slice(payload);
    if f.len() < MIN_FRAME {
        f.resize(MIN_FRAME, 0);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let f = build([1; 6], [2; 6], TYPE_ARP, &[9, 9]);
        assert_eq!(f.len(), MIN_FRAME);
        let p = parse(&f).unwrap();
        assert_eq!(p.dst, [1; 6]);
        assert_eq!(p.src, [2; 6]);
        assert_eq!(p.ethertype, TYPE_ARP);
        assert_eq!(&p.payload[..2], &[9, 9]);
    }
}
