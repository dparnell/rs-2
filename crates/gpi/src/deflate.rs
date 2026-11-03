//! A small zlib (RFC 1950) / deflate (RFC 1951) compressor for the
//! PostScript device's images (FlateDecode): LZ77 over a 32 KB window
//! with hash chains, coded in one block with the fixed Huffman codes.
//! Screen pictures (long runs, repeated rows) shrink to a few per cent.

use alloc::vec;
use alloc::vec::Vec;

const WINDOW: usize = 32768;
const HASH_BITS: u32 = 15;
const MAX_CHAIN: usize = 48;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    /// `n` bits of `v`, least significant first.
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code (`n` bits, most significant first).
    fn code(&mut self, c: u32, n: u32) {
        let mut r = 0;
        for i in 0..n {
            r |= ((c >> i) & 1) << (n - 1 - i);
        }
        self.put(r, n);
    }

    fn flush(&mut self) {
        if self.n > 0 {
            self.out.push(self.acc as u8);
            self.acc = 0;
            self.n = 0;
        }
    }
}

/// A literal/length symbol with the fixed codes.
fn sym(b: &mut Bits, s: u32) {
    match s {
        0..=143 => b.code(0x30 + s, 8),
        144..=255 => b.code(0x190 + s - 144, 9),
        256..=279 => b.code(s - 256, 7),
        _ => b.code(0xc0 + s - 280, 8),
    }
}

fn length(b: &mut Bits, len: usize) {
    let i = LEN_BASE.iter().rposition(|&x| x as usize <= len).unwrap_or(0);
    sym(b, 257 + i as u32);
    b.put((len - LEN_BASE[i] as usize) as u32, LEN_EXTRA[i] as u32);
}

fn distance(b: &mut Bits, d: usize) {
    let i = DIST_BASE.iter().rposition(|&x| x as usize <= d).unwrap_or(0);
    b.code(i as u32, 5);
    b.put((d - DIST_BASE[i] as usize) as u32, DIST_EXTRA[i] as u32);
}

fn hash(d: &[u8], i: usize) -> usize {
    let v = (d[i] as u32) << 16 | (d[i + 1] as u32) << 8 | d[i + 2] as u32;
    (v.wrapping_mul(0x9e37_79b1) >> (32 - HASH_BITS)) as usize
}

/// Raw deflate data of `d`.
pub fn deflate(d: &[u8]) -> Vec<u8> {
    let mut b = Bits { out: Vec::with_capacity(d.len() / 4 + 16), acc: 0, n: 0 };
    // BFINAL, BTYPE 01 (fixed codes).
    b.put(1, 1);
    b.put(1, 2);
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; WINDOW];
    let mut i = 0;
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
        if i + MIN_MATCH <= d.len() {
            let h = hash(d, i);
            prev[i % WINDOW] = head[h];
            head[h] = i;
        }
    };
    while i < d.len() {
        let (mut best, mut at) = (0, 0);
        if i + MIN_MATCH <= d.len() {
            let mut cand = head[hash(d, i)];
            let mut chain = 0;
            let max = (d.len() - i).min(MAX_MATCH);
            while cand != usize::MAX && i - cand <= WINDOW && chain < MAX_CHAIN {
                if d[cand + best] == d[i + best] {
                    let mut l = 0;
                    while l < max && d[cand + l] == d[i + l] {
                        l += 1;
                    }
                    if l > best {
                        best = l;
                        at = cand;
                        if l == max {
                            break;
                        }
                    }
                }
                let p = prev[cand % WINDOW];
                if p == usize::MAX || p >= cand {
                    break;
                }
                cand = p;
                chain += 1;
            }
        }
        if best >= MIN_MATCH {
            length(&mut b, best);
            distance(&mut b, i - at);
            for k in 0..best {
                insert(&mut head, &mut prev, i + k);
            }
            i += best;
        } else {
            sym(&mut b, d[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    sym(&mut b, 256);
    b.flush();
    b.out
}

/// A zlib stream (FlateDecode's format) of `d`.
pub fn zlib(d: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend_from_slice(&deflate(d));
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in d.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    out.extend_from_slice(&(b << 16 | a).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An inflater for fixed-code blocks (what `deflate` writes).
    fn inflate_fixed(src: &[u8]) -> Vec<u8> {
        let mut pos = 0usize;
        let bit = |pos: &mut usize| -> u32 {
            let v = (src[*pos / 8] >> (*pos % 8)) & 1;
            *pos += 1;
            v as u32
        };
        let bits = |pos: &mut usize, n: u32| -> u32 {
            let mut v = 0;
            for i in 0..n {
                v |= bit(pos) << i;
            }
            v
        };
        let code = |pos: &mut usize, n: u32| -> u32 {
            let mut v = 0;
            for _ in 0..n {
                v = v << 1 | bit(pos);
            }
            v
        };
        assert_eq!(bits(&mut pos, 3), 0b011);
        let mut out = Vec::new();
        loop {
            // Literal/length: 7, 8 or 9 bits.
            let mut c = code(&mut pos, 7);
            let s = if c <= 0x17 {
                c + 256
            } else {
                c = c << 1 | bit(&mut pos);
                if (0x30..=0xbf).contains(&c) {
                    c - 0x30
                } else if (0xc0..=0xc7).contains(&c) {
                    c - 0xc0 + 280
                } else {
                    c = c << 1 | bit(&mut pos);
                    c - 0x190 + 144
                }
            };
            match s {
                0..=255 => out.push(s as u8),
                256 => break,
                _ => {
                    let k = (s - 257) as usize;
                    let len = LEN_BASE[k] as usize + bits(&mut pos, LEN_EXTRA[k] as u32) as usize;
                    let dk = code(&mut pos, 5) as usize;
                    let dist = DIST_BASE[dk] as usize + bits(&mut pos, DIST_EXTRA[dk] as u32) as usize;
                    for _ in 0..len {
                        let b = out[out.len() - dist];
                        out.push(b);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn round_trip() {
        let mut d = Vec::new();
        for i in 0..70000u32 {
            d.push(if i % 1000 < 600 { 0x2e } else { (i * 7 % 251) as u8 });
        }
        d.extend_from_slice(b"short tail");
        let z = deflate(&d);
        assert!(z.len() < d.len() / 3);
        assert_eq!(inflate_fixed(&z), d);
        assert_eq!(inflate_fixed(&deflate(b"")), b"");
        assert_eq!(inflate_fixed(&deflate(b"ab")), b"ab");
        let zl = zlib(b"abc");
        assert_eq!(&zl[..2], &[0x78, 0x01]);
        assert_eq!(&zl[zl.len() - 4..], &0x024d_0127u32.to_be_bytes());
    }
}
