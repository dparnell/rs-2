//! The files of a zip archive (stored or deflated, in no directories).

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u32,
    n: u32,
}

impl Bits<'_> {
    fn bits(&mut self, want: u32) -> Option<u32> {
        while self.n < want {
            self.buf |= (*self.d.get(self.pos)? as u32) << self.n;
            self.pos += 1;
            self.n += 8;
        }
        let v = self.buf & ((1 << want) - 1);
        self.buf >>= want;
        self.n -= want;
        Some(v)
    }
}

/// Canonical Huffman codes: how many there are of each length, and the
/// symbols in their order.
struct Huffman {
    count: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut count = [0; 16];
        let mut symbols = Vec::new();
        for l in 1..16 {
            let v = (0..lengths.len()).filter(|&i| lengths[i] as usize == l).map(|i| i as u16);
            let before = symbols.len();
            symbols.extend(v);
            count[l] = (symbols.len() - before) as u16;
        }
        Huffman { count, symbols }
    }

    fn decode(&self, b: &mut Bits) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for l in 1..16 {
            code |= b.bits(1)? as i32;
            let count = self.count[l] as i32;
            if code - count < first {
                return self.symbols.get((index + code - first) as usize).copied();
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

fn inflate(d: &[u8]) -> Option<Vec<u8>> {
    const LEN: [(u16, u32); 29] = [
        (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0), (11, 1), (13, 1), (15, 1), (17, 1), (19, 2), (23, 2),
        (27, 2), (31, 2), (35, 3), (43, 3), (51, 3), (59, 3), (67, 4), (83, 4), (99, 4), (115, 4), (131, 5), (163, 5), (195, 5),
        (227, 5), (258, 0),
    ];
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut b = Bits { d, pos: 0, buf: 0, n: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = b.bits(1)? == 1;
        let (lit, dist) = match b.bits(2)? {
            0 => {
                // Stored: from the next whole byte.
                (b.buf, b.n) = (0, 0);
                let n = u16::from_le_bytes(d.get(b.pos..b.pos + 2)?.try_into().ok()?) as usize;
                out.extend_from_slice(d.get(b.pos + 4..b.pos + 4 + n)?);
                b.pos += 4 + n;
                if last {
                    return Some(out);
                }
                continue;
            }
            1 => {
                let l: Vec<u8> = (0..288).map(|i| match i { 0..144 => 8, 144..256 => 9, 256..280 => 7, _ => 8 }).collect();
                (Huffman::new(&l), Huffman::new(&[5; 30]))
            }
            2 => {
                let (nl, nd, nc) = (b.bits(5)? as usize + 257, b.bits(5)? as usize + 1, b.bits(4)? as usize + 4);
                let mut cl = [0u8; 19];
                for &i in &ORDER[..nc] {
                    cl[i] = b.bits(3)? as u8;
                }
                let h = Huffman::new(&cl);
                let mut l = Vec::new();
                while l.len() < nl + nd {
                    match h.decode(&mut b)? {
                        16 => {
                            let p = *l.last()?;
                            let n = b.bits(2)? as usize + 3;
                            l.extend(std::iter::repeat_n(p, n));
                        }
                        17 => l.extend(std::iter::repeat_n(0, b.bits(3)? as usize + 3)),
                        18 => l.extend(std::iter::repeat_n(0, b.bits(7)? as usize + 11)),
                        s => l.push(s as u8),
                    }
                }
                (Huffman::new(&l[..nl]), Huffman::new(l.get(nl..nl + nd)?))
            }
            _ => return None,
        };
        loop {
            match lit.decode(&mut b)? {
                256 => break,
                s @ 0..256 => out.push(s as u8),
                s => {
                    let (base, extra) = *LEN.get(s as usize - 257)?;
                    let n = base as usize + b.bits(extra)? as usize;
                    let c = dist.decode(&mut b)? as u32;
                    let (base, extra) = if c < 4 { (c + 1, 0) } else { (((2 + c % 2) << (c / 2 - 1)) + 1, c / 2 - 1) };
                    let back = base as usize + b.bits(extra)? as usize;
                    for _ in 0..n {
                        out.push(*out.get(out.len().checked_sub(back)?)?);
                    }
                }
            }
        }
        if last {
            return Some(out);
        }
    }
}

/// The archive's files: names (no directories) and contents.
pub fn unpack(z: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let u16_at = |o: usize| Some(u16::from_le_bytes(z.get(o..o + 2)?.try_into().ok()?) as usize);
    let u32_at = |o: usize| Some(u32::from_le_bytes(z.get(o..o + 4)?.try_into().ok()?) as usize);
    let end = (0..z.len().checked_sub(21)?).rev().find(|&o| z[o..].starts_with(b"PK\x05\x06"))?;
    let (count, mut p) = (u16_at(end + 10)?, u32_at(end + 16)?);
    let mut v = Vec::new();
    for _ in 0..count {
        if !z.get(p..)?.starts_with(b"PK\x01\x02") {
            return None;
        }
        let (method, csize, size) = (u16_at(p + 10)?, u32_at(p + 20)?, u32_at(p + 24)?);
        let (n, x, c, local) = (u16_at(p + 28)?, u16_at(p + 30)?, u16_at(p + 32)?, u32_at(p + 42)?);
        let name = String::from_utf8_lossy(z.get(p + 46..p + 46 + n)?).into_owned();
        p += 46 + n + x + c;
        if name.contains(['/', '\\', ':']) || name.starts_with("..") {
            return None;
        }
        let at = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let data = z.get(at..at + csize)?;
        let d = match method {
            0 => data.to_vec(),
            8 => inflate(data)?,
            _ => return None,
        };
        if d.len() != size {
            return None;
        }
        v.push((name, d));
    }
    Some(v)
}
