//! Byte order helpers and the Internet checksum (RFC 1071).

use crate::Ipv4;

pub fn be16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}

pub fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_be_bytes());
}

pub fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_be_bytes());
}

pub fn ip_at(b: &[u8], o: usize) -> Ipv4 {
    Ipv4([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Ones' complement sum of `data` added to `acc` (not yet folded).
pub fn sum(mut acc: u32, data: &[u8]) -> u32 {
    let mut chunks = data.chunks_exact(2);
    for c in &mut chunks {
        acc += u16::from_be_bytes([c[0], c[1]]) as u32;
        // Fold early so long buffers cannot overflow.
        if acc > 0xffff_0000 {
            acc = (acc & 0xffff) + (acc >> 16);
        }
    }
    if let [last] = chunks.remainder() {
        acc += (*last as u32) << 8;
    }
    acc
}

/// Fold and complement a sum into the checksum field value.
pub fn finish(mut acc: u32) -> u16 {
    while acc > 0xffff {
        acc = (acc & 0xffff) + (acc >> 16);
    }
    !(acc as u16)
}

/// Checksum of `data`.
pub fn checksum(data: &[u8]) -> u16 {
    finish(sum(0, data))
}

/// Sum of the TCP/UDP pseudo-header.
pub fn pseudo(src: Ipv4, dst: Ipv4, proto: u8, len: usize) -> u32 {
    let acc = sum(sum(0, &src.0), &dst.0);
    acc + proto as u32 + len as u32
}

/// Checksum of a transport segment including the pseudo-header.
pub fn transport_checksum(src: Ipv4, dst: Ipv4, proto: u8, seg: &[u8]) -> u16 {
    finish(sum(pseudo(src, dst, proto, seg.len()), seg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc1071_example() {
        // RFC 1071 section 3: 00 01 f2 03 f4 f5 f6 f7 sums to ddf2.
        let d = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        assert_eq!(finish(sum(0, &d)), !0xddf2);
        // A header with its checksum filled in sums to zero.
        let mut h = [0x45, 0, 0, 0x54, 0, 0, 0x40, 0, 0x40, 1, 0, 0, 10, 0, 2, 15, 10, 0, 2, 2];
        let c = checksum(&h);
        put16(&mut h, 10, c);
        assert_eq!(checksum(&h), 0);
    }

    #[test]
    fn odd_length() {
        assert_eq!(checksum(&[0xab]), !0xab00);
    }
}
