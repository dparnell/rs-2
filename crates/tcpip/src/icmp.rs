//! ICMP (RFC 792): echo request/reply and the error messages the stack
//! sends (destination unreachable) or reacts to.

use alloc::vec::Vec;

use crate::wire::{be16, checksum, put16};

pub const ECHO_REPLY: u8 = 0;
pub const DEST_UNREACHABLE: u8 = 3;
pub const ECHO_REQUEST: u8 = 8;
pub const TIME_EXCEEDED: u8 = 11;

// Destination unreachable codes.
pub const UNREACH_NET: u8 = 0;
pub const UNREACH_HOST: u8 = 1;
pub const UNREACH_PROTOCOL: u8 = 2;
pub const UNREACH_PORT: u8 = 3;

pub struct Message<'a> {
    pub kind: u8,
    pub code: u8,
    /// The 4 bytes after the checksum (identifier and sequence for echo).
    pub rest: [u8; 4],
    pub data: &'a [u8],
}

impl Message<'_> {
    pub fn ident(&self) -> u16 {
        be16(&self.rest, 0)
    }

    pub fn seq(&self) -> u16 {
        be16(&self.rest, 2)
    }
}

pub fn parse(b: &[u8]) -> Option<Message<'_>> {
    if b.len() < 8 || checksum(b) != 0 {
        return None;
    }
    Some(Message { kind: b[0], code: b[1], rest: [b[4], b[5], b[6], b[7]], data: &b[8..] })
}

pub fn build(kind: u8, code: u8, rest: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + data.len());
    v.extend_from_slice(&[kind, code, 0, 0]);
    v.extend_from_slice(&rest);
    v.extend_from_slice(data);
    let c = checksum(&v);
    put16(&mut v, 2, c);
    v
}

pub fn echo(kind: u8, ident: u16, seq: u16, data: &[u8]) -> Vec<u8> {
    let i = ident.to_be_bytes();
    let s = seq.to_be_bytes();
    build(kind, 0, [i[0], i[1], s[0], s[1]], data)
}

/// Destination unreachable for `datagram` (its IP header and the first 8
/// payload bytes are quoted).
pub fn unreachable(code: u8, datagram: &[u8], ihl: usize) -> Vec<u8> {
    let n = (ihl + 8).min(datagram.len());
    build(DEST_UNREACHABLE, code, [0; 4], &datagram[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_roundtrip() {
        let m = echo(ECHO_REQUEST, 0x1234, 7, b"abc");
        let p = parse(&m).unwrap();
        assert_eq!((p.kind, p.code, p.ident(), p.seq(), p.data), (ECHO_REQUEST, 0, 0x1234, 7, &b"abc"[..]));
    }
}
