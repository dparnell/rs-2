//! A QR code encoder (ISO/IEC 18004, model 2) for one alphanumeric
//! segment: versions 1-40, all four error correction levels, the mask
//! with the lowest penalty. No allocation: the symbol and its codewords
//! live in [`QrCode`].

pub const MAX_SIZE: usize = 177;
const MAX_CODEWORDS: usize = 3706;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Ecc {
    L = 0,
    M = 1,
    Q = 2,
    H = 3,
}

impl Ecc {
    fn format_bits(self) -> u32 {
        [1, 0, 3, 2][self as usize]
    }
}

const ECC_PER_BLOCK: [[u8; 41]; 4] = [
    [0, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
    [0, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [0, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];

const NUM_BLOCKS: [[u8; 41]; 4] = [
    [0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
    [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
    [0, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
    [0, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];

/// The alphanumeric character set (also base45's).
pub const ALNUM: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

/// Modules available for data and error correction (everything but the
/// function patterns), in bits.
fn raw_data_modules(ver: usize) -> usize {
    let mut r = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let n = ver / 7 + 2;
        r -= (25 * n - 10) * n - 55;
        if ver >= 7 {
            r -= 36;
        }
    }
    r
}

/// Data codewords of a version at a level.
pub fn data_codewords(ver: usize, ecc: Ecc) -> usize {
    raw_data_modules(ver) / 8 - ECC_PER_BLOCK[ecc as usize][ver] as usize * NUM_BLOCKS[ecc as usize][ver] as usize
}

/// Bits an alphanumeric segment of `n` characters takes in a version.
fn segment_bits(ver: usize, n: usize) -> usize {
    let count_bits = match ver {
        1..=9 => 9,
        10..=26 => 11,
        _ => 13,
    };
    4 + count_bits + n / 2 * 11 + n % 2 * 6
}

/// The largest alphanumeric text that fits a version at a level.
pub fn alnum_capacity(ver: usize, ecc: Ecc) -> usize {
    let bits = data_codewords(ver, ecc) * 8;
    let mut n = (bits.saturating_sub(segment_bits(ver, 0))) * 2 / 11 + 1;
    while n > 0 && segment_bits(ver, n) > bits {
        n -= 1;
    }
    n
}

fn gf_mul(x: u8, y: u8) -> u8 {
    let mut z: u32 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11d);
        z ^= ((y as u32 >> i) & 1) * x as u32;
    }
    z as u8
}

/// Reed-Solomon generator polynomial of degree `deg` (leading 1 left out).
fn rs_divisor(deg: usize, out: &mut [u8; 30]) {
    out.fill(0);
    out[deg - 1] = 1;
    let mut root = 1u8;
    for _ in 0..deg {
        for j in 0..deg {
            out[j] = gf_mul(out[j], root);
            if j + 1 < deg {
                out[j] ^= out[j + 1];
            }
        }
        root = gf_mul(root, 2);
    }
}

fn rs_remainder(data: &[u8], div: &[u8; 30], deg: usize, out: &mut [u8]) {
    out[..deg].fill(0);
    for &b in data {
        let factor = b ^ out[0];
        out.copy_within(1..deg, 0);
        out[deg - 1] = 0;
        for i in 0..deg {
            out[i] ^= gf_mul(div[i], factor);
        }
    }
}

fn alignment_positions(ver: usize, out: &mut [usize; 7]) -> usize {
    if ver == 1 {
        return 0;
    }
    let n = ver / 7 + 2;
    let size = ver * 4 + 17;
    let step = if ver == 32 { 26 } else { (ver * 4 + n * 2 + 1) / (n * 2 - 2) * 2 };
    out[0] = 6;
    let mut pos = size - 7;
    for i in (1..n).rev() {
        out[i] = pos;
        pos -= step;
    }
    n
}

pub struct QrCode {
    size: usize,
    version: usize,
    /// Bit 0: dark; bit 1: function pattern.
    m: [u8; MAX_SIZE * MAX_SIZE],
    data: [u8; MAX_CODEWORDS],
    all: [u8; MAX_CODEWORDS],
}

const DARK: u8 = 1;
const FUNC: u8 = 2;

impl QrCode {
    pub const fn new() -> QrCode {
        QrCode { size: 0, version: 0, m: [0; MAX_SIZE * MAX_SIZE], data: [0; MAX_CODEWORDS], all: [0; MAX_CODEWORDS] }
    }

    /// Modules per side (0 before a successful `encode`).
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn version(&self) -> usize {
        self.version
    }

    pub fn dark(&self, x: usize, y: usize) -> bool {
        self.m[y * MAX_SIZE + x] & DARK != 0
    }

    fn set(&mut self, x: usize, y: usize, dark: bool) {
        self.m[y * MAX_SIZE + x] = FUNC | dark as u8;
    }

    /// Encode `text` (characters of [`ALNUM`] only) in the smallest version
    /// from `min_ver` to `max_ver` that holds it at level `ecc`, raising
    /// the level while the text still fits that version. The version, or
    /// None if the text does not fit or has other characters.
    pub fn encode(&mut self, text: &[u8], ecc: Ecc, min_ver: usize, max_ver: usize) -> Option<usize> {
        let vals = |c: u8| ALNUM.iter().position(|&a| a == c);
        if text.iter().any(|&c| vals(c).is_none()) {
            return None;
        }
        let ver = (min_ver.max(1)..=max_ver.min(40)).find(|&v| segment_bits(v, text.len()) <= data_codewords(v, ecc) * 8)?;
        let mut ecc = ecc;
        for e in [Ecc::M, Ecc::Q, Ecc::H] {
            if e > ecc && segment_bits(ver, text.len()) <= data_codewords(ver, e) * 8 {
                ecc = e;
            }
        }
        self.version = ver;
        self.size = ver * 4 + 17;

        // Data codewords: mode, count, characters, terminator, padding.
        let cap = data_codewords(ver, ecc);
        self.data[..cap].fill(0);
        let mut bitlen = 0usize;
        let put = |data: &mut [u8], bitlen: &mut usize, v: u32, n: usize| {
            for i in (0..n).rev() {
                if v >> i & 1 != 0 {
                    data[*bitlen / 8] |= 0x80 >> (*bitlen % 8);
                }
                *bitlen += 1;
            }
        };
        let count_bits = segment_bits(ver, 0) - 4;
        put(&mut self.data, &mut bitlen, 0b0010, 4);
        put(&mut self.data, &mut bitlen, text.len() as u32, count_bits);
        for pair in text.chunks(2) {
            let a = vals(pair[0]).unwrap() as u32;
            match pair.get(1) {
                Some(&c) => put(&mut self.data, &mut bitlen, a * 45 + vals(c).unwrap() as u32, 11),
                None => put(&mut self.data, &mut bitlen, a, 6),
            }
        }
        let term = (cap * 8 - bitlen).min(4);
        put(&mut self.data, &mut bitlen, 0, term);
        let pad = (8 - bitlen % 8) % 8;
        put(&mut self.data, &mut bitlen, 0, pad);
        let mut i = bitlen / 8;
        let mut p = 0xec;
        while i < cap {
            self.data[i] = p;
            p ^= 0xec ^ 0x11;
            i += 1;
        }

        // Error correction blocks, interleaved.
        let blocks = NUM_BLOCKS[ecc as usize][ver] as usize;
        let ecc_len = ECC_PER_BLOCK[ecc as usize][ver] as usize;
        let raw = raw_data_modules(ver) / 8;
        let short = blocks - raw % blocks;
        let short_len = raw / blocks; // data + ecc of a short block
        let short_data = short_len - ecc_len;
        let mut div = [0u8; 30];
        rs_divisor(ecc_len, &mut div);
        let start = |j: usize| j * short_data + j.saturating_sub(short);
        let dlen = |j: usize| short_data + (j >= short) as usize;
        let mut r = 0;
        for i in 0..=short_data {
            for j in 0..blocks {
                if i < dlen(j) {
                    self.all[r] = self.data[start(j) + i];
                    r += 1;
                }
            }
        }
        let mut rem = [0u8; 30];
        let ecc_base = r;
        for j in 0..blocks {
            rs_remainder(&self.data[start(j)..start(j) + dlen(j)], &div, ecc_len, &mut rem);
            for i in 0..ecc_len {
                self.all[ecc_base + i * blocks + j] = rem[i];
            }
        }
        let total = ecc_base + blocks * ecc_len;
        debug_assert_eq!(total, raw);

        // The symbol.
        for y in 0..self.size {
            self.m[y * MAX_SIZE..y * MAX_SIZE + self.size].fill(0);
        }
        self.function_patterns(ecc);
        self.codewords(total);
        let mut best = (u32::MAX, 0);
        for mask in 0..8 {
            self.apply_mask(mask);
            self.format_bits(ecc, mask);
            let p = self.penalty();
            if p < best.0 {
                best = (p, mask);
            }
            self.apply_mask(mask);
        }
        self.apply_mask(best.1);
        self.format_bits(ecc, best.1);
        Some(ver)
    }

    fn function_patterns(&mut self, ecc: Ecc) {
        let size = self.size;
        for i in 0..size {
            self.set(6, i, i % 2 == 0);
            self.set(i, 6, i % 2 == 0);
        }
        for (cx, cy) in [(3, 3), (size - 4, 3), (3, size - 4)] {
            for dy in -4i32..=4 {
                for dx in -4i32..=4 {
                    let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                    if (0..size as i32).contains(&x) && (0..size as i32).contains(&y) {
                        let d = dx.abs().max(dy.abs());
                        self.set(x as usize, y as usize, d != 2 && d != 4);
                    }
                }
            }
        }
        let mut pos = [0usize; 7];
        let n = alignment_positions(self.version, &mut pos);
        for i in 0..n {
            for j in 0..n {
                if (i == 0 && j == 0) || (i == 0 && j == n - 1) || (i == n - 1 && j == 0) {
                    continue;
                }
                for dy in -2i32..=2 {
                    for dx in -2i32..=2 {
                        let d = dx.abs().max(dy.abs());
                        self.set((pos[i] as i32 + dx) as usize, (pos[j] as i32 + dy) as usize, d != 1);
                    }
                }
            }
        }
        self.format_bits(ecc, 0);
        if self.version >= 7 {
            let mut rem = self.version as u32;
            for _ in 0..12 {
                rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
            }
            let bits = (self.version as u32) << 12 | rem;
            for i in 0..18 {
                let dark = bits >> i & 1 != 0;
                let a = size - 11 + i % 3;
                let b = i / 3;
                self.set(a, b, dark);
                self.set(b, a, dark);
            }
        }
    }

    fn format_bits(&mut self, ecc: Ecc, mask: u32) {
        let data = ecc.format_bits() << 3 | mask;
        let mut rem = data;
        for _ in 0..10 {
            rem = (rem << 1) ^ ((rem >> 9) * 0x537);
        }
        let bits = (data << 10 | rem) ^ 0x5412;
        let bit = |i: usize| bits >> i & 1 != 0;
        let size = self.size;
        for i in 0..=5 {
            self.set(8, i, bit(i));
        }
        self.set(8, 7, bit(6));
        self.set(8, 8, bit(7));
        self.set(7, 8, bit(8));
        for i in 9..15 {
            self.set(14 - i, 8, bit(i));
        }
        for i in 0..8 {
            self.set(size - 1 - i, 8, bit(i));
        }
        for i in 8..15 {
            self.set(8, size - 15 + i, bit(i));
        }
        self.set(8, size - 8, true);
    }

    fn codewords(&mut self, total: usize) {
        let size = self.size;
        let mut i = 0;
        let mut right = size as i32 - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..size {
                for j in 0..2 {
                    let x = (right - j) as usize;
                    let upward = (right + 1) & 2 == 0;
                    let y = if upward { size - 1 - vert } else { vert };
                    let k = y * MAX_SIZE + x;
                    if self.m[k] & FUNC == 0 && i < total * 8 {
                        if self.all[i / 8] >> (7 - i % 8) & 1 != 0 {
                            self.m[k] |= DARK;
                        }
                        i += 1;
                    }
                }
            }
            right -= 2;
        }
    }

    fn apply_mask(&mut self, mask: u32) {
        for y in 0..self.size {
            for x in 0..self.size {
                let flip = match mask {
                    0 => (x + y) % 2 == 0,
                    1 => y % 2 == 0,
                    2 => x % 3 == 0,
                    3 => (x + y) % 3 == 0,
                    4 => (x / 3 + y / 2) % 2 == 0,
                    5 => x * y % 2 + x * y % 3 == 0,
                    6 => (x * y % 2 + x * y % 3) % 2 == 0,
                    _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
                };
                let k = y * MAX_SIZE + x;
                if flip && self.m[k] & FUNC == 0 {
                    self.m[k] ^= DARK;
                }
            }
        }
    }

    fn penalty(&self) -> u32 {
        let size = self.size;
        let mut p = 0u32;
        let at = |x: usize, y: usize, horiz: bool| if horiz { self.dark(x, y) } else { self.dark(y, x) };
        for horiz in [true, false] {
            for y in 0..size {
                // Runs of five or more.
                let mut run = 0;
                let mut last = false;
                for x in 0..size {
                    let d = at(x, y, horiz);
                    if x > 0 && d == last {
                        run += 1;
                    } else {
                        if run >= 5 {
                            p += 3 + (run - 5);
                        }
                        run = 1;
                        last = d;
                    }
                }
                if run >= 5 {
                    p += 3 + (run - 5);
                }
                // Finder-like 1:1:3:1:1 with four light modules on a side
                // (the area outside the symbol counts as light).
                const PAT: [bool; 7] = [true, false, true, true, true, false, true];
                for x in -4i32..size as i32 - 6 {
                    let get = |i: i32| (0..size as i32).contains(&i) && at(i as usize, y, horiz);
                    if (0..7).all(|k| get(x + k) == PAT[k as usize]) {
                        let before = (1..=4).all(|k| !get(x - k));
                        let after = (7..11).all(|k| !get(x + k));
                        if before || after {
                            p += 40;
                        }
                    }
                }
            }
        }
        let mut dark = 0;
        for y in 0..size {
            for x in 0..size {
                let d = self.dark(x, y);
                dark += d as usize;
                if x + 1 < size && y + 1 < size && d == self.dark(x + 1, y) && d == self.dark(x, y + 1) && d == self.dark(x + 1, y + 1) {
                    p += 3;
                }
            }
        }
        let total = size * size;
        let k = ((dark * 20).abs_diff(total * 10) + total - 1) / total - 1;
        p + k as u32 * 10
    }
}

impl Default for QrCode {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacities() {
        // Known alphanumeric capacities.
        assert_eq!(alnum_capacity(1, Ecc::L), 25);
        assert_eq!(alnum_capacity(1, Ecc::H), 10);
        assert_eq!(alnum_capacity(10, Ecc::M), 311);
        assert_eq!(alnum_capacity(40, Ecc::L), 4296);
        assert_eq!(alnum_capacity(40, Ecc::H), 1852);
    }

    #[test]
    fn reed_solomon_hello_world_1q() {
        // The data and error correction codewords of "HELLO WORLD", 1-Q.
        let data = [32u8, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236];
        let mut div = [0u8; 30];
        rs_divisor(13, &mut div);
        let mut rem = [0u8; 30];
        rs_remainder(&data, &div, 13, &mut rem);
        assert_eq!(&rem[..13], &[168, 72, 22, 82, 217, 54, 156, 0, 46, 15, 180, 122, 16]);
        let mut q = QrCode::new();
        assert_eq!(q.encode(b"HELLO WORLD", Ecc::Q, 1, 40), Some(1));
        assert_eq!(&q.data[..13], &data);
    }

    #[test]
    fn version_choice() {
        let mut q = QrCode::new();
        let text = vec![b'A'; 4296];
        assert_eq!(q.encode(&text, Ecc::L, 1, 40), Some(40));
        assert_eq!(q.size(), 177);
        let text = vec![b'A'; 4297];
        assert_eq!(q.encode(&text, Ecc::L, 1, 40), None);
        assert_eq!(q.encode(b"abc", Ecc::L, 1, 40), None);
    }
}
