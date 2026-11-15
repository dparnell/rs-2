//! zlib streams (RFC 1950/1951) with one fixed-Huffman deflate block:
//! LZ77 matches found through hash chains, no dynamic code tables. Logs
//! shrink to roughly a third, which is what the QR code needs, and the
//! compressor is small and needs no allocation.

const WINDOW: usize = 32768;
const HASH_BITS: u32 = 13;
const HASH: usize = 1 << HASH_BITS;
const MAX_CHAIN: usize = 96;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// The compressor's working memory (about 160 KB: keep it in a static).
pub struct Deflate {
    /// Most recent position + 1 with each hash (0 = none).
    head: [u32; HASH],
    /// The previous position + 1 with the same hash, by position mod WINDOW.
    prev: [u32; WINDOW],
}

struct Bits<'a> {
    out: &'a mut [u8],
    len: usize,
    acc: u64,
    n: u32,
    ok: bool,
}

impl Bits<'_> {
    fn byte(&mut self, b: u8) {
        match self.out.get_mut(self.len) {
            Some(x) => {
                *x = b;
                self.len += 1;
            }
            None => self.ok = false,
        }
    }

    /// `count` bits of `v`, least significant first.
    fn put(&mut self, v: u32, count: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.byte(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code (sent most significant bit first).
    fn code(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    fn flush(&mut self) {
        if self.n > 0 {
            self.byte(self.acc as u8);
            self.acc = 0;
            self.n = 0;
        }
    }

    /// A literal/length symbol in the fixed code.
    fn sym(&mut self, s: u32) {
        match s {
            0..=143 => self.code(0x30 + s, 8),
            144..=255 => self.code(0x190 + s - 144, 9),
            256..=279 => self.code(s - 256, 7),
            _ => self.code(0xc0 + s - 280, 8),
        }
    }

    fn matched(&mut self, len: usize, dist: usize) {
        let i = LEN_BASE.iter().rposition(|&b| b as usize <= len).unwrap();
        self.sym(257 + i as u32);
        self.put((len - LEN_BASE[i] as usize) as u32, LEN_EXTRA[i] as u32);
        let d = DIST_BASE.iter().rposition(|&b| b as usize <= dist).unwrap();
        self.code(d as u32, 5);
        self.put((dist - DIST_BASE[d] as usize) as u32, DIST_EXTRA[d] as u32);
    }
}

fn hash(b: &[u8]) -> usize {
    let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
    (v.wrapping_mul(2654435761) >> (32 - HASH_BITS)) as usize
}

pub fn adler32(d: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in d.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

impl Deflate {
    pub const fn new() -> Deflate {
        Deflate { head: [0; HASH], prev: [0; WINDOW] }
    }

    fn insert(&mut self, d: &[u8], i: usize) {
        if i + MIN_MATCH <= d.len() {
            let h = hash(&d[i..]);
            self.prev[i % WINDOW] = self.head[h];
            self.head[h] = i as u32 + 1;
        }
    }

    fn longest(&self, d: &[u8], i: usize) -> (usize, usize) {
        let (mut best, mut dist) = (0, 0);
        if i + MIN_MATCH > d.len() {
            return (0, 0);
        }
        let max = (d.len() - i).min(MAX_MATCH);
        let mut cand = self.head[hash(&d[i..])];
        for _ in 0..MAX_CHAIN {
            if cand == 0 {
                break;
            }
            let c = cand as usize - 1;
            if i - c > WINDOW || c >= i {
                break;
            }
            if d[c + best] == d[i + best] || best == 0 {
                let mut n = 0;
                while n < max && d[c + n] == d[i + n] {
                    n += 1;
                }
                if n > best {
                    best = n;
                    dist = i - c;
                    if n == max {
                        break;
                    }
                }
            }
            let next = self.prev[c % WINDOW];
            // Older entries only: a slot reused for a newer position ends
            // the chain.
            if next as usize >= cand as usize {
                break;
            }
            cand = next;
        }
        (best, dist)
    }

    /// Compress `data` as a zlib stream into `out`: its length, or None if
    /// `out` is too small.
    pub fn zlib(&mut self, data: &[u8], out: &mut [u8]) -> Option<usize> {
        self.head.fill(0);
        let mut b = Bits { out, len: 0, acc: 0, n: 0, ok: true };
        b.byte(0x78);
        b.byte(0x01);
        b.put(1, 1); // last block
        b.put(1, 2); // fixed Huffman codes
        let mut i = 0;
        while i < data.len() {
            let (len, dist) = self.longest(data, i);
            if len >= MIN_MATCH {
                b.matched(len, dist);
                for j in i..i + len {
                    self.insert(data, j);
                }
                i += len;
            } else {
                b.sym(data[i] as u32);
                self.insert(data, i);
                i += 1;
            }
            if !b.ok {
                return None;
            }
        }
        b.sym(256);
        b.flush();
        for x in adler32(data).to_be_bytes() {
            b.byte(x);
        }
        b.ok.then_some(b.len)
    }
}

impl Default for Deflate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fixed-Huffman-only inflater, to check the compressor on its own
    /// (python3's zlib checks it independently in `pack_python`).
    pub fn inflate_fixed(z: &[u8]) -> Vec<u8> {
        assert_eq!(&z[..2], &[0x78, 0x01]);
        let mut pos = 16usize; // bit position
        let bit = |p: &mut usize| -> u32 {
            let v = (z[*p / 8] >> (*p % 8)) & 1;
            *p += 1;
            v as u32
        };
        let bits = |p: &mut usize, n: u32| -> u32 {
            let mut v = 0;
            for i in 0..n {
                v |= bit(p) << i;
            }
            v
        };
        assert_eq!(bits(&mut pos, 3), 0b011);
        let mut out = Vec::new();
        loop {
            // Read a fixed literal/length code.
            let mut code = 0u32;
            let mut len = 0;
            let sym = loop {
                code = code << 1 | bit(&mut pos);
                len += 1;
                match len {
                    7 if code <= 0x17 => break code + 256,
                    8 if (0x30..=0xbf).contains(&code) => break code - 0x30,
                    8 if (0xc0..=0xc7).contains(&code) => break code - 0xc0 + 280,
                    9 if (0x190..=0x1ff).contains(&code) => break code - 0x190 + 144,
                    _ => assert!(len < 9),
                }
            };
            match sym {
                0..=255 => out.push(sym as u8),
                256 => break,
                _ => {
                    let i = (sym - 257) as usize;
                    let l = LEN_BASE[i] as usize + bits(&mut pos, LEN_EXTRA[i] as u32) as usize;
                    let mut dc = 0;
                    for _ in 0..5 {
                        dc = dc << 1 | bit(&mut pos);
                    }
                    let d = DIST_BASE[dc as usize] as usize + bits(&mut pos, DIST_EXTRA[dc as usize] as u32) as usize;
                    for _ in 0..l {
                        out.push(out[out.len() - d]);
                    }
                }
            }
        }
        let end = pos.div_ceil(8);
        assert_eq!(u32::from_be_bytes(z[end..end + 4].try_into().unwrap()), adler32(&out));
        out
    }

    pub fn sample_log() -> Vec<u8> {
        let mut s = String::new();
        for i in 0..400 {
            s += &format!("xhci: port {} connected, speed {} slot {}\nnvme: namespace 1: {} sectors\n", i % 7, i % 3, i, 1000 + i * 37);
        }
        s.into_bytes()
    }

    #[test]
    fn roundtrip() {
        let mut z = Box::new(Deflate::new());
        let mut out = vec![0u8; 1 << 20];
        for data in [Vec::new(), b"a".to_vec(), b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(), sample_log(), (0..70000u32).map(|i| (i * 7919 % 251) as u8).collect()] {
            let n = z.zlib(&data, &mut out).unwrap();
            assert_eq!(inflate_fixed(&out[..n]), data);
        }
        let log = sample_log();
        let n = z.zlib(&log, &mut out).unwrap();
        assert!(n * 3 < log.len(), "{n} of {}", log.len());
        assert!(z.zlib(&log, &mut out[..10]).is_none());
    }
}
