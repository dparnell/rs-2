//! Deflate (RFC 1951), decompressed in pieces for files too large to hold,
//! and zip's CRC-32. Table-driven, and works on any input: damaged data
//! gives an error.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;

const WINDOW: usize = 32 * 1024;
/// What is handed on at once, about.
const PIECE: usize = 64 * 1024;
/// Codes this short are found with one look.
const FAST: u32 = 10;

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193,
    12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// The data is not deflate data, or ends early.
    Damaged,
    /// What the receiver of the data said.
    Sink(E),
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u64,
    n: u32,
}

impl Bits<'_> {
    fn fill(&mut self) {
        while self.n <= 56 && self.pos < self.d.len() {
            self.buf |= (self.d[self.pos] as u64) << self.n;
            self.pos += 1;
            self.n += 8;
        }
    }

    fn bits(&mut self, want: u32) -> Option<u32> {
        if self.n < want {
            self.fill();
            if self.n < want {
                return None;
            }
        }
        let v = (self.buf & ((1 << want) - 1)) as u32;
        self.buf >>= want;
        self.n -= want;
        Some(v)
    }
}

/// Canonical Huffman codes: short ones in a table, all by length.
struct Huffman {
    /// By the next FAST bits: symbol << 4 | length; 0: a longer code.
    fast: Vec<u16>,
    count: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Huffman> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for l in 1..16 {
            code = (code + count[l - 1] as u32) << 1;
            next[l] = code;
            // More codes than there can be.
            if code + count[l] as u32 > 1 << l {
                return None;
            }
            offs[l] = offs[l - 1] + count[l - 1];
        }
        let mut symbols = alloc::vec![0u16; lengths.len()];
        let mut fast = alloc::vec![0u16; 1 << FAST];
        for (s, &l) in lengths.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let l = l as u32;
            symbols[offs[l as usize] as usize] = s as u16;
            offs[l as usize] += 1;
            let c = next[l as usize];
            next[l as usize] += 1;
            if l <= FAST {
                // The bits come with the code's highest first.
                let mut i = (c.reverse_bits() >> (32 - l)) as usize;
                while i < 1 << FAST {
                    fast[i] = (s as u16) << 4 | l as u16;
                    i += 1 << l;
                }
            }
        }
        Some(Huffman { fast, count, symbols })
    }

    #[inline]
    fn decode(&self, b: &mut Bits) -> Option<u32> {
        if b.n < 15 {
            b.fill();
        }
        let f = self.fast[(b.buf & ((1 << FAST) - 1)) as usize];
        if f != 0 {
            let l = (f & 15) as u32;
            if l > b.n {
                return None;
            }
            b.buf >>= l;
            b.n -= l;
            return Some((f >> 4) as u32);
        }
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for l in 1..16 {
            code |= b.bits(1)? as i32;
            let count = self.count[l] as i32;
            if code - count < first {
                return self.symbols.get((index + code - first) as usize).map(|&s| s as u32);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

fn tables(b: &mut Bits) -> Option<(Huffman, Huffman)> {
    let nlit = b.bits(5)? as usize + 257;
    let ndist = b.bits(5)? as usize + 1;
    let ncode = b.bits(4)? as usize + 4;
    let mut l = [0u8; 19];
    for &i in &ORDER[..ncode] {
        l[i] = b.bits(3)? as u8;
    }
    let h = Huffman::new(&l)?;
    let mut lengths = [0u8; 288 + 32];
    let mut n = 0;
    while n < nlit + ndist {
        let (v, times) = match h.decode(b)? {
            s @ 0..=15 => (s as u8, 1),
            16 if n > 0 => (lengths[n - 1], 3 + b.bits(2)? as usize),
            17 => (0, 3 + b.bits(3)? as usize),
            18 => (0, 11 + b.bits(7)? as usize),
            _ => return None,
        };
        lengths.get_mut(n..n + times)?.fill(v);
        n += times;
    }
    if n != nlit + ndist {
        return None;
    }
    Some((Huffman::new(&lengths[..nlit])?, Huffman::new(&lengths[nlit..n])?))
}

/// Decompress `d`, handing the data to `sink` in order, in pieces.
pub fn inflate<E>(d: &[u8], sink: &mut dyn FnMut(&[u8]) -> Result<(), E>) -> Result<(), Error<E>> {
    let mut b = Bits { d, pos: 0, buf: 0, n: 0 };
    // The last WINDOW bytes handed on, and what has not been.
    let mut o: Vec<u8> = Vec::with_capacity(WINDOW + PIECE + 300);
    let mut sent = 0;
    let bad = || Error::Damaged;
    macro_rules! flush {
        ($all:expr) => {
            if $all || o.len() >= WINDOW + PIECE {
                sink(&o[sent..]).map_err(Error::Sink)?;
                let keep = o.len().min(WINDOW);
                let from = o.len() - keep;
                o.copy_within(from.., 0);
                o.truncate(keep);
                sent = keep;
            }
        };
    }
    loop {
        let last = b.bits(1).ok_or_else(bad)? == 1;
        match b.bits(2).ok_or_else(bad)? {
            0 => {
                // Stored: from the next byte.
                b.bits(b.n % 8).ok_or_else(bad)?;
                let len = b.bits(16).ok_or_else(bad)?;
                if b.bits(16).ok_or_else(bad)? != !len & 0xffff {
                    return Err(bad());
                }
                for _ in 0..len {
                    let c = b.bits(8).ok_or_else(bad)? as u8;
                    o.push(c);
                    flush!(false);
                }
            }
            kind @ (1 | 2) => {
                let (lit, dist) = if kind == 1 {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    (Huffman::new(&l).ok_or_else(bad)?, Huffman::new(&[5u8; 30]).ok_or_else(bad)?)
                } else {
                    tables(&mut b).ok_or_else(bad)?
                };
                loop {
                    let s = lit.decode(&mut b).ok_or_else(bad)?;
                    if s < 256 {
                        o.push(s as u8);
                    } else if s == 256 {
                        break;
                    } else {
                        let i = s as usize - 257;
                        let extra = *LEN_EXTRA.get(i).ok_or_else(bad)? as u32;
                        let len = LEN_BASE[i] as usize + b.bits(extra).ok_or_else(bad)? as usize;
                        let k = dist.decode(&mut b).ok_or_else(bad)? as usize;
                        let extra = *DIST_EXTRA.get(k).ok_or_else(bad)? as u32;
                        let back = DIST_BASE[k] as usize + b.bits(extra).ok_or_else(bad)? as usize;
                        let from = o.len().checked_sub(back).ok_or_else(bad)?;
                        if back >= len {
                            o.extend_from_within(from..from + len);
                        } else {
                            for j in from..from + len {
                                let c = o[j];
                                o.push(c);
                            }
                        }
                    }
                    flush!(false);
                }
            }
            _ => return Err(bad()),
        }
        if last {
            flush!(true);
            let _ = sent;
            return Ok(());
        }
    }
}

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC: [u32; 256] = crc_table();

/// CRC-32 of data that continues data with the CRC `crc` (0 to begin).
pub fn crc32(crc: u32, d: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in d {
        c = CRC[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(d: &[u8]) -> Result<Vec<u8>, Error<()>> {
        let mut v = Vec::new();
        inflate(d, &mut |p| {
            v.extend_from_slice(p);
            Ok(())
        })?;
        Ok(v)
    }

    #[test]
    fn crc() {
        assert_eq!(crc32(0, b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(crc32(0, b"1234"), b"56789"), 0xcbf4_3926);
    }

    #[test]
    fn stored_and_fixed() {
        assert_eq!(all(&[1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c']).unwrap(), b"abc");
        // zlib's raw deflate of "hello hello hello hello"
        assert_eq!(all(&[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01]).unwrap(), b"hello hello hello hello");
    }

    #[test]
    fn damaged() {
        assert_eq!(all(&[]), Err(Error::Damaged));
        assert_eq!(all(&[7]), Err(Error::Damaged));
        assert_eq!(all(&[1, 3, 0, 0, 0, 1]), Err(Error::Damaged));
        // A distance before the beginning.
        assert_eq!(all(&[0x03, 0x02, 0x00]).is_err(), true);
        let r = inflate(&[1, 1, 0, 0xfe, 0xff, 9], &mut |_| Err(5));
        assert_eq!(r, Err(Error::Sink(5)));
        // Never panics.
        let mut x = 1u32;
        for n in 0..20_000usize {
            let d: Vec<u8> = (0..n % 90)
                .map(|_| {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (x >> 24) as u8
                })
                .collect();
            let _ = all(&d);
        }
    }
}
