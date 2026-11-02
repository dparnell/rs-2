//! JPEG (ITU T.81) files: baseline, extended and progressive Huffman
//! decoding with 8-bit samples, and a baseline JFIF encoder.
//!
//! A file is a sequence of marker segments (0xFF, code, 16-bit length):
//! DQT quantization tables, DHT Huffman tables, SOF frame headers (size,
//! components with their sampling factors and table numbers), DRI
//! restart interval, SOS scans followed by entropy-coded data (0xFF bytes
//! in it stuffed with 0x00, RSTn markers between restart intervals).
//! Coefficients come in zig-zag order, 8×8 blocks grouped in MCUs when a
//! scan has several components. Progressive files send the coefficients
//! in several scans: spectral selection (a band of coefficients) and
//! successive approximation (the high bits first, then one bit per
//! refinement scan), with end-of-band runs spanning blocks.
//!
//! Colour: one component is grey, three are YCbCr (or RGB when an Adobe
//! APP14 segment says transform 0), four are Adobe CMYK (inverted) or
//! YCCK (transform 2). Chroma is upsampled by replication. The IDCT and
//! FDCT are straightforward separable integer matrix products.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Error, Image, be16};

/// ZIGZAG[k]: the natural (row-major) index of the k-th coefficient.
const ZIGZAG: [u8; 64] = zigzag();

const fn zigzag() -> [u8; 64] {
    let mut z = [0u8; 64];
    let (mut k, mut s) = (0, 0);
    while s < 15 {
        // Diagonal s: row + col = s, walked up-right when s is even.
        let mut i = 0;
        while i <= s {
            let (r, c) = if s % 2 == 0 { (s - i, i) } else { (i, s - i) };
            if r < 8 && c < 8 {
                z[k] = (r * 8 + c) as u8;
                k += 1;
            }
            i += 1;
        }
        s += 1;
    }
    z
}

/// cos(kπ/16) for k = 0..8.
const COS16: [f64; 9] = [1.0, 0.980785280403230, 0.923879532511287, 0.831469612302545, 0.707106781186548, 0.555570233019602, 0.382683432365090, 0.195090322016128, 0.0];

const fn cos16(m: usize) -> f64 {
    let m = m % 32;
    let m = if m > 16 { 32 - m } else { m };
    if m > 8 { -COS16[16 - m] } else { COS16[m] }
}

/// T[x][u] = 4096 · C(u) · cos((2x+1)uπ/16), C(0) = 1/√2, C(u) = 1.
const T: [[i32; 8]; 8] = dct_table();

const fn dct_table() -> [[i32; 8]; 8] {
    let mut t = [[0i32; 8]; 8];
    let mut x = 0;
    while x < 8 {
        let mut u = 0;
        while u < 8 {
            let c = if u == 0 { COS16[4] } else { 1.0 };
            let v = 4096.0 * c * cos16((2 * x + 1) * u);
            t[x][u] = if v < 0.0 { (v - 0.5) as i32 } else { (v + 0.5) as i32 };
            u += 1;
        }
        x += 1;
    }
    t
}

/// In-place inverse DCT of dequantized coefficients (natural order) to
/// samples with the level shift and clamping.
fn idct(c: &[i32; 64], out: &mut [u8; 64]) {
    let mut r = [0i32; 64];
    for v in 0..8 {
        for x in 0..8 {
            let mut s = 0i32;
            for u in 0..8 { s += c[v * 8 + u] * T[x][u]; }
            r[v * 8 + x] = (s + 256) >> 9;
        }
    }
    for x in 0..8 {
        for y in 0..8 {
            let mut s = 0i64;
            for v in 0..8 { s += r[v * 8 + x] as i64 * T[y][v] as i64; }
            out[y * 8 + x] = (((s + (1 << 16)) >> 17) + 128).clamp(0, 255) as u8;
        }
    }
}

/// Forward DCT and quantization: `q` in natural order.
fn fdct(f: &[i32; 64], q: &[u16; 64], out: &mut [i32; 64]) {
    let mut r = [0i32; 64];
    for y in 0..8 {
        for u in 0..8 {
            let mut s = 0i32;
            for x in 0..8 { s += f[y * 8 + x] * T[x][u]; }
            r[y * 8 + u] = s;
        }
    }
    for u in 0..8 {
        for v in 0..8 {
            let mut s = 0i64;
            for y in 0..8 { s += r[y * 8 + u] as i64 * T[y][v] as i64; }
            let d = (q[v * 8 + u] as i64) << 26;
            // Within what the baseline Huffman tables can code.
            out[v * 8 + u] = (if s >= 0 { (s + d / 2) / d } else { -((-s + d / 2) / d) }).clamp(-1023, 1023) as i32;
        }
    }
}

#[derive(Clone)]
struct Huff {
    /// 9-bit lookahead: (length, value), length 0 = longer code.
    fast: Vec<(u8, u8)>,
    maxcode: [i32; 18],
    valptr: [i32; 17],
    mincode: [i32; 17],
    vals: Vec<u8>,
}

impl Huff {
    fn new(counts: &[u8; 16], vals: &[u8]) -> Result<Huff, Error> {
        let mut h = Huff { fast: vec![(0, 0); 512], maxcode: [-1; 18], valptr: [0; 17], mincode: [0; 17], vals: vals.to_vec() };
        let (mut code, mut k) = (0i32, 0usize);
        for l in 1..=16 {
            let n = counts[l - 1] as usize;
            h.valptr[l] = k as i32;
            h.mincode[l] = code;
            for _ in 0..n {
                let v = *vals.get(k).ok_or(Error::Corrupt)?;
                if l <= 9 {
                    let shift = 9 - l;
                    let base = (code as usize) << shift;
                    for e in h.fast.get_mut(base..base + (1 << shift)).ok_or(Error::Corrupt)? { *e = (l as u8, v); }
                }
                code += 1;
                k += 1;
            }
            h.maxcode[l] = if n > 0 { code - 1 } else { -1 };
            if code > 1 << l { return Err(Error::Corrupt); }
            code <<= 1;
        }
        h.maxcode[17] = i32::MAX;
        Ok(h)
    }
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
}

impl<'a> Bits<'a> {
    fn fill(&mut self) {
        while self.n <= 56 {
            let b = match self.d.get(self.pos) {
                Some(&0xFF) => match self.d.get(self.pos + 1) {
                    Some(0) => { self.pos += 2; 0xFF }
                    // A marker: feed zeros from here on.
                    _ => 0,
                },
                Some(&b) => { self.pos += 1; b }
                None => 0,
            };
            self.acc |= (b as u64) << (56 - self.n);
            self.n += 8;
        }
    }

    fn peek(&mut self, k: u32) -> u32 {
        if self.n < k { self.fill(); }
        (self.acc >> (64 - k)) as u32
    }

    fn skip(&mut self, k: u32) {
        self.acc <<= k;
        self.n -= k;
    }

    fn get(&mut self, k: u32) -> u32 {
        if k == 0 { return 0; }
        let v = self.peek(k);
        self.skip(k);
        v
    }

    /// A k-bit magnitude extended to a signed value (T.81 F.2.2.1).
    fn receive_extend(&mut self, k: u32) -> i32 {
        if k == 0 { return 0; }
        let v = self.get(k.min(16)) as i32;
        if v < 1 << (k - 1) { v - (1 << k) + 1 } else { v }
    }

    fn decode(&mut self, h: &Huff) -> Result<u8, Error> {
        let p = self.peek(16);
        let (l, v) = h.fast[(p >> 7) as usize];
        if l > 0 {
            self.skip(l as u32);
            return Ok(v);
        }
        for l in 10..=16 {
            let code = (p >> (16 - l)) as i32;
            if code <= h.maxcode[l] {
                self.skip(l as u32);
                let i = h.valptr[l] + code - h.mincode[l];
                return h.vals.get(i as usize).copied().ok_or(Error::Corrupt);
            }
        }
        Err(Error::Corrupt)
    }

    /// Skips to just after the next RSTn marker (restart interval end).
    fn restart(&mut self) {
        self.acc = 0;
        self.n = 0;
        while self.pos + 1 < self.d.len() {
            if self.d[self.pos] == 0xFF && (0xD0..=0xD7).contains(&self.d[self.pos + 1]) {
                self.pos += 2;
                return;
            }
            if self.d[self.pos] == 0xFF && self.d[self.pos + 1] != 0 && self.d[self.pos + 1] != 0xFF { return; }
            self.pos += 1;
        }
    }
}

struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    /// Blocks across and down, padded to whole MCUs.
    bw: usize,
    bh: usize,
    coefs: Vec<i16>,
    dc_pred: i32,
}

struct Frame {
    width: usize,
    height: usize,
    progressive: bool,
    comps: Vec<Comp>,
    hmax: usize,
    vmax: usize,
    mcux: usize,
    mcuy: usize,
}

pub fn decode(data: &[u8]) -> Result<Image, Error> {
    if !data.starts_with(&[0xFF, 0xD8]) { return Err(Error::NotThisFormat); }
    let mut qt: [Option<[u16; 64]>; 4] = [None; 4];
    let mut dc: [Option<Huff>; 4] = [None, None, None, None];
    let mut ac: [Option<Huff>; 4] = [None, None, None, None];
    let mut frame: Option<Frame> = None;
    let mut restart = 0usize;
    let mut adobe: Option<u8> = None;
    let mut eobrun;
    let mut at = 2;
    let mut scans = 0;
    loop {
        // Find the next marker (tolerating fill bytes and junk).
        while at < data.len() && data[at] != 0xFF { at += 1; }
        while at < data.len() && data[at] == 0xFF { at += 1; }
        let Some(&m) = data.get(at) else { break };
        at += 1;
        if m == 0xD9 { break; }
        if (0xD0..=0xD7).contains(&m) || m == 0x01 { continue; }
        let len = be16(data, at).ok_or(Error::Truncated)? as usize;
        if len < 2 { return Err(Error::Corrupt); }
        let seg = data.get(at + 2..at + len).ok_or(Error::Truncated)?;
        at += len;
        match m {
            0xC0 | 0xC1 | 0xC2 => {
                if frame.is_some() { return Err(Error::Corrupt); }
                if seg.len() < 6 || seg[0] != 8 { return Err(Error::Unsupported); }
                let height = be16(seg, 1).unwrap_or(0) as usize;
                let width = be16(seg, 3).unwrap_or(0) as usize;
                let n = seg[5] as usize;
                if height == 0 { return Err(Error::Unsupported); }
                if width == 0 || !matches!(n, 1 | 3 | 4) { return Err(Error::Unsupported); }
                let mut comps = Vec::new();
                for i in 0..n {
                    let c = seg.get(6 + i * 3..9 + i * 3).ok_or(Error::Truncated)?;
                    let (h, v) = ((c[1] >> 4) as usize, (c[1] & 15) as usize);
                    if !(1..=4).contains(&h) || !(1..=4).contains(&v) || c[2] > 3 { return Err(Error::Corrupt); }
                    comps.push(Comp { id: c[0], h, v, tq: c[2] as usize, bw: 0, bh: 0, coefs: Vec::new(), dc_pred: 0 });
                }
                let hmax = comps.iter().map(|c| c.h).max().unwrap_or(1);
                let vmax = comps.iter().map(|c| c.v).max().unwrap_or(1);
                let (mcux, mcuy) = (width.div_ceil(8 * hmax), height.div_ceil(8 * vmax));
                let total: usize = comps.iter().map(|c| mcux * c.h * mcuy * c.v * 64).sum();
                if total > 1 << 27 { return Err(Error::Unsupported); }
                for c in &mut comps {
                    c.bw = mcux * c.h;
                    c.bh = mcuy * c.v;
                    c.coefs = vec![0; c.bw * c.bh * 64];
                }
                frame = Some(Frame { width, height, progressive: m == 0xC2, comps, hmax, vmax, mcux, mcuy });
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err(Error::Unsupported),
            0xC4 => {
                let mut p = 0;
                while p < seg.len() {
                    let tc = seg[p];
                    let counts: [u8; 16] = seg.get(p + 1..p + 17).ok_or(Error::Truncated)?.try_into().map_err(|_| Error::Corrupt)?;
                    let n: usize = counts.iter().map(|&c| c as usize).sum();
                    let vals = seg.get(p + 17..p + 17 + n).ok_or(Error::Truncated)?;
                    let h = Huff::new(&counts, vals)?;
                    let id = (tc & 3) as usize;
                    if tc >> 4 == 0 { dc[id] = Some(h) } else { ac[id] = Some(h) }
                    p += 17 + n;
                }
            }
            0xDB => {
                let mut p = 0;
                while p < seg.len() {
                    let (pq, tq) = (seg[p] >> 4, (seg[p] & 3) as usize);
                    let mut q = [0u16; 64];
                    for k in 0..64 {
                        let v = if pq == 0 { *seg.get(p + 1 + k).ok_or(Error::Truncated)? as u16 } else { be16(seg, p + 1 + 2 * k).ok_or(Error::Truncated)? };
                        q[ZIGZAG[k] as usize] = v;
                    }
                    qt[tq] = Some(q);
                    p += 1 + 64 * if pq == 0 { 1 } else { 2 };
                }
            }
            0xDD => restart = be16(seg, 0).ok_or(Error::Truncated)? as usize,
            0xEE => if seg.len() >= 12 && seg.starts_with(b"Adobe") { adobe = Some(seg[11]); },
            0xDA => {
                let f = frame.as_mut().ok_or(Error::Corrupt)?;
                let ns = *seg.first().ok_or(Error::Truncated)? as usize;
                let mut sc = Vec::new();
                for i in 0..ns {
                    let c = seg.get(1 + i * 2..3 + i * 2).ok_or(Error::Truncated)?;
                    let ci = f.comps.iter().position(|x| x.id == c[0]).ok_or(Error::Corrupt)?;
                    sc.push((ci, (c[1] >> 4) as usize & 3, (c[1] & 3) as usize));
                }
                let p = seg.get(1 + ns * 2..4 + ns * 2).ok_or(Error::Truncated)?;
                let (ss, se, ah, al) = (p[0] as usize, p[1] as usize, (p[2] >> 4) as u32, (p[2] & 15) as u32);
                if ns == 0 || ns > 4 || se > 63 || ss > se || al > 13 { return Err(Error::Corrupt); }
                if !f.progressive && (ss != 0 || se != 63 || ah != 0 || al != 0) { return Err(Error::Corrupt); }
                if f.progressive && ss > 0 && ns != 1 { return Err(Error::Corrupt); }
                let mut bits = Bits { d: data, pos: at, acc: 0, n: 0 };
                for c in &mut f.comps { c.dc_pred = 0; }
                eobrun = 0;
                let ctx = ScanCtx { ss, se, ah, al, progressive: f.progressive };
                // The units of this scan: MCUs, or single blocks.
                let (ux, uy) = if ns == 1 {
                    let c = &f.comps[sc[0].0];
                    ((f.width * c.h).div_ceil(f.hmax).div_ceil(8), (f.height * c.v).div_ceil(f.vmax).div_ceil(8))
                } else {
                    (f.mcux, f.mcuy)
                };
                let mut count = 0usize;
                'units: for my in 0..uy {
                    for mx in 0..ux {
                        if restart > 0 && count > 0 && count % restart == 0 {
                            bits.restart();
                            for c in &mut f.comps { c.dc_pred = 0; }
                            eobrun = 0;
                        }
                        count += 1;
                        for &(ci, td, ta) in &sc {
                            let c = &mut f.comps[ci];
                            let (bh, bv) = if ns == 1 { (1, 1) } else { (c.h, c.v) };
                            for by in 0..bv {
                                for bx in 0..bh {
                                    let (x, y) = (mx * bh + bx, my * bv + by);
                                    let b = (y * c.bw + x) * 64;
                                    let Some(blk) = c.coefs.get_mut(b..b + 64) else { continue };
                                    let r = decode_block(&mut bits, blk, &mut c.dc_pred, &mut eobrun, dc[td].as_ref(), ac[ta].as_ref(), &ctx);
                                    if let Err(e) = r {
                                        // Damaged data: keep what was decoded.
                                        if scans == 0 && count == 1 { return Err(e); }
                                        break 'units;
                                    }
                                }
                            }
                        }
                    }
                }
                scans += 1;
                // The entropy-coded data ends at the next real marker.
                at = bits.pos.min(data.len());
                while at + 1 < data.len() && !(data[at] == 0xFF && data[at + 1] != 0 && !(0xD0..=0xD7).contains(&data[at + 1])) { at += 1; }
            }
            _ => {}
        }
    }
    let f = frame.ok_or(Error::Truncated)?;
    if scans == 0 { return Err(Error::Truncated); }
    // Dequantize and transform every block into component planes.
    let mut planes = Vec::new();
    for c in &f.comps {
        let q = qt[c.tq].ok_or(Error::Corrupt)?;
        let pw = c.bw * 8;
        let mut plane = vec![0u8; pw * c.bh * 8];
        let mut deq = [0i32; 64];
        let mut out = [0u8; 64];
        for by in 0..c.bh {
            for bx in 0..c.bw {
                let b = &c.coefs[(by * c.bw + bx) * 64..][..64];
                for k in 0..64 { deq[k] = (b[k] as i32 * q[k] as i32).clamp(-32767, 32767); }
                idct(&deq, &mut out);
                for y in 0..8 {
                    let at = (by * 8 + y) * pw + bx * 8;
                    plane[at..at + 8].copy_from_slice(&out[y * 8..y * 8 + 8]);
                }
            }
        }
        planes.push((plane, pw));
    }
    let (w, h) = (f.width as u32, f.height as u32);
    let sample = |i: usize, x: usize, y: usize| -> i32 {
        let c = &f.comps[i];
        let (p, pw) = (&planes[i].0, planes[i].1);
        p[(y * c.v / f.vmax) * pw + x * c.h / f.hmax] as i32
    };
    if f.comps.len() == 1 {
        let mut img = Image::alloc(w, h, 8)?;
        img.palette = (0..=255u8).map(|v| [v; 3]).collect();
        for y in 0..f.height {
            for x in 0..f.width { img.set_index(x as u32, y as u32, sample(0, x, y) as u8); }
        }
        return Ok(img);
    }
    let mut img = Image::alloc(w, h, 24)?;
    let ids: Vec<u8> = f.comps.iter().map(|c| c.id).collect();
    let rgb = f.comps.len() == 3 && (adobe == Some(0) || ids == b"RGB");
    for y in 0..f.height {
        for x in 0..f.width {
            let (a, b, c) = (sample(0, x, y), sample(1, x, y), sample(2, x, y));
            let px = if rgb { [a as u8, b as u8, c as u8] } else if f.comps.len() == 3 || adobe == Some(2) { ycc(a, b, c) } else { [a as u8, b as u8, c as u8] };
            let px = if f.comps.len() == 4 {
                // Adobe CMYK/YCCK is stored inverted: 255 = no ink.
                let k = sample(3, x, y);
                let inv = adobe == Some(2);
                px.map(|v| { let v = if inv { 255 - v as i32 } else { v as i32 }; (v * k / 255) as u8 })
            } else { px };
            img.set_rgb(x as u32, y as u32, px);
        }
    }
    Ok(img)
}

fn ycc(y: i32, cb: i32, cr: i32) -> [u8; 3] {
    let (cb, cr) = (cb - 128, cr - 128);
    let r = y + ((91881 * cr + 32768) >> 16);
    let g = y - ((22554 * cb + 46802 * cr - 32768) >> 16);
    let b = y + ((116130 * cb + 32768) >> 16);
    [r.clamp(0, 255) as u8, g.clamp(0, 255) as u8, b.clamp(0, 255) as u8]
}

struct ScanCtx {
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
    progressive: bool,
}

fn decode_block(b: &mut Bits, blk: &mut [i16], pred: &mut i32, eobrun: &mut u32, dc: Option<&Huff>, ac: Option<&Huff>, s: &ScanCtx) -> Result<(), Error> {
    if !s.progressive {
        let t = b.decode(dc.ok_or(Error::Corrupt)?)? as u32;
        if t > 11 { return Err(Error::Corrupt); }
        *pred += b.receive_extend(t);
        blk[0] = *pred as i16;
        let ac = ac.ok_or(Error::Corrupt)?;
        let mut k = 1;
        while k < 64 {
            let rs = b.decode(ac)?;
            let (r, sz) = ((rs >> 4) as usize, (rs & 15) as u32);
            if sz == 0 {
                if r != 15 { break; }
                k += 16;
                continue;
            }
            k += r;
            if k > 63 { return Err(Error::Corrupt); }
            blk[ZIGZAG[k] as usize] = b.receive_extend(sz) as i16;
            k += 1;
        }
        return Ok(());
    }
    if s.ss == 0 {
        // DC scans (spectral selection is DC only).
        if s.ah == 0 {
            let t = b.decode(dc.ok_or(Error::Corrupt)?)? as u32;
            if t > 11 { return Err(Error::Corrupt); }
            *pred += b.receive_extend(t);
            blk[0] = (*pred << s.al) as i16;
        } else if b.get(1) != 0 {
            blk[0] |= 1 << s.al;
        }
        return Ok(());
    }
    let ac = ac.ok_or(Error::Corrupt)?;
    if s.ah == 0 {
        // AC first pass.
        if *eobrun > 0 {
            *eobrun -= 1;
            return Ok(());
        }
        let mut k = s.ss;
        while k <= s.se {
            let rs = b.decode(ac)?;
            let (r, sz) = ((rs >> 4) as u32, (rs & 15) as u32);
            if sz == 0 {
                if r < 15 {
                    *eobrun = (1 << r) - 1 + b.get(r);
                    break;
                }
                k += 16;
                continue;
            }
            k += r as usize;
            if k > 63 { return Err(Error::Corrupt); }
            blk[ZIGZAG[k] as usize] = (b.receive_extend(sz) << s.al) as i16;
            k += 1;
        }
        return Ok(());
    }
    // AC refinement: one more bit for coefficients already nonzero, and
    // new coefficients of magnitude 1 placed after runs of zeros.
    let p1 = 1i16 << s.al;
    let m1 = -1i16 << s.al;
    let mut k = s.ss;
    let refine = |b: &mut Bits, c: &mut i16| {
        if b.get(1) != 0 && *c & p1 == 0 {
            *c += if *c >= 0 { p1 } else { m1 };
        }
    };
    if *eobrun == 0 {
        while k <= s.se {
            let rs = b.decode(ac)?;
            let (mut r, sz) = ((rs >> 4) as i32, (rs & 15) as u32);
            let mut val = 0i16;
            if sz != 0 {
                if sz != 1 { return Err(Error::Corrupt); }
                val = if b.get(1) != 0 { p1 } else { m1 };
            } else if r != 15 {
                *eobrun = (1 << r) + b.get(r as u32);
                break;
            }
            while k <= s.se {
                let z = ZIGZAG[k] as usize;
                if blk[z] != 0 {
                    refine(b, &mut blk[z]);
                } else {
                    if r == 0 {
                        if val != 0 { blk[z] = val; }
                        break;
                    }
                    r -= 1;
                }
                k += 1;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= s.se {
            let z = ZIGZAG[k] as usize;
            if blk[z] != 0 { refine(b, &mut blk[z]); }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

// The example tables of T.81 Annex K.

const LUMA_Q: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62,
    18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

const CHROMA_Q: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];

/// The AC symbols in code order: these lead, then for each run the sizes
/// not yet listed in increasing order (runs 0..15, sizes 1..10).
const AC_LUMA_LEAD: &[u8] = &[
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1,
    0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26,
];
const AC_CHROMA_LEAD: &[u8] = &[
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42,
    0x91, 0xA1, 0xB1, 0xC1, 0x09, 0x23, 0x33, 0x52, 0xF0, 0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1, 0x17, 0x18, 0x19,
    0x1A, 0x26,
];

/// The full 162-symbol list: the lead symbols, then the remaining ones
/// in increasing order (which is how Annex K lists them).
fn ac_vals(lead: &[u8]) -> Vec<u8> {
    let mut v = lead.to_vec();
    for r in 0..16u8 {
        for s in 1..=10u8 {
            let sym = r << 4 | s;
            if !v.contains(&sym) { v.push(sym); }
        }
    }
    v
}

struct Enc {
    codes: [(u16, u8); 256],
}

impl Enc {
    fn new(counts: &[u8; 16], vals: &[u8]) -> Enc {
        let mut e = Enc { codes: [(0, 0); 256] };
        let (mut code, mut k) = (0u16, 0);
        for l in 1..=16u8 {
            for _ in 0..counts[l as usize - 1] {
                e.codes[vals[k] as usize] = (code, l);
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        e
    }
}

struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, v: u32, k: u32) {
        if k == 0 { return; }
        self.acc = self.acc << k | (v & ((1 << k) - 1));
        self.n += k;
        while self.n >= 8 {
            let b = (self.acc >> (self.n - 8)) as u8;
            self.out.push(b);
            if b == 0xFF { self.out.push(0); }
            self.n -= 8;
        }
        self.acc &= (1 << self.n) - 1;
    }

    fn flush(&mut self) {
        if self.n > 0 { let k = 8 - self.n; self.put((1 << k) - 1, k); }
    }
}

fn magnitude(v: i32) -> (u32, u32) {
    let a = v.unsigned_abs();
    let s = 32 - a.leading_zeros();
    (s, if v < 0 { (v - 1) as u32 & ((1 << s) - 1) } else { v as u32 })
}

fn encode_block(w: &mut BitWriter, c: &[i32; 64], pred: &mut i32, dc: &Enc, ac: &Enc) {
    let d = c[0] - *pred;
    *pred = c[0];
    let (s, v) = magnitude(d);
    let (code, len) = dc.codes[s as usize];
    w.put(code as u32, len as u32);
    w.put(v, s);
    let mut run = 0;
    for k in 1..64 {
        let x = c[ZIGZAG[k] as usize];
        if x == 0 { run += 1; continue; }
        while run > 15 {
            let (code, len) = ac.codes[0xF0];
            w.put(code as u32, len as u32);
            run -= 16;
        }
        let (s, v) = magnitude(x);
        let (code, len) = ac.codes[(run << 4 | s) as usize];
        w.put(code as u32, len as u32);
        w.put(v, s);
        run = 0;
    }
    if run > 0 {
        let (code, len) = ac.codes[0];
        w.put(code as u32, len as u32);
    }
}

fn scaled(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = quality.clamp(1, 100) as u32;
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    base.map(|b| ((b as u32 * scale + 50) / 100).clamp(1, 255) as u16)
}

fn segment(o: &mut Vec<u8>, marker: u8, body: &[u8]) {
    o.extend_from_slice(&[0xFF, marker]);
    o.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    o.extend_from_slice(body);
}

/// Encodes a baseline JFIF file: grey for images with a grey palette,
/// otherwise YCbCr, 4:2:0 below quality 90 and 4:4:4 from 90 up.
pub fn encode(img: &Image, quality: u8) -> Vec<u8> {
    let grey = img.is_grey();
    let sub = if grey || quality >= 90 { 1 } else { 2 };
    let (w, h) = (img.width as usize, img.height as usize);
    let lq = scaled(&LUMA_Q, quality);
    let cq = scaled(&CHROMA_Q, quality);
    let mut o = vec![0xFF, 0xD8];
    segment(&mut o, 0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    let mut dqt = vec![0u8];
    dqt.extend((0..64).map(|k| lq[ZIGZAG[k] as usize] as u8));
    if !grey {
        dqt.push(1);
        dqt.extend((0..64).map(|k| cq[ZIGZAG[k] as usize] as u8));
    }
    segment(&mut o, 0xDB, &dqt);
    let mut sof = vec![8];
    sof.extend_from_slice(&(h as u16).to_be_bytes());
    sof.extend_from_slice(&(w as u16).to_be_bytes());
    if grey {
        sof.extend_from_slice(&[1, 1, 0x11, 0]);
    } else {
        sof.extend_from_slice(&[3, 1, (sub << 4 | sub) as u8, 0, 2, 0x11, 1, 3, 0x11, 1]);
    }
    segment(&mut o, 0xC0, &sof);
    let acl = ac_vals(AC_LUMA_LEAD);
    let acc = ac_vals(AC_CHROMA_LEAD);
    let mut dht = Vec::new();
    for (tc, bits, vals) in [(0x00u8, &DC_LUMA_BITS, &DC_VALS[..]), (0x10, &AC_LUMA_BITS, &acl[..]), (0x01, &DC_CHROMA_BITS, &DC_VALS[..]), (0x11, &AC_CHROMA_BITS, &acc[..])] {
        if grey && tc & 1 != 0 { continue; }
        dht.push(tc);
        dht.extend_from_slice(bits);
        dht.extend_from_slice(vals);
    }
    segment(&mut o, 0xC4, &dht);
    segment(&mut o, 0xDA, if grey { &[1, 1, 0x00, 0, 63, 0] } else { &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0] });
    let (dcl, acl, dcc, acc) = (Enc::new(&DC_LUMA_BITS, &DC_VALS), Enc::new(&AC_LUMA_BITS, &acl), Enc::new(&DC_CHROMA_BITS, &DC_VALS), Enc::new(&AC_CHROMA_BITS, &acc));
    // Colour conversion with edge replication.
    let px = |x: usize, y: usize| -> [i32; 3] {
        let c = img.rgb(x.min(w - 1) as u32, y.min(h - 1) as u32);
        let (r, g, b) = (c[0] as i32, c[1] as i32, c[2] as i32);
        let yy = (19595 * r + 38470 * g + 7471 * b + 32768) >> 16;
        let cb = ((-11059 * r - 21709 * g + 32768 * b + 32768) >> 16) + 128;
        let cr = ((32768 * r - 27439 * g - 5329 * b + 32768) >> 16) + 128;
        [yy, cb, cr]
    };
    let mut bw = BitWriter { out: o, acc: 0, n: 0 };
    let mut preds = [0i32; 3];
    let mut f = [0i32; 64];
    let mut q = [0i32; 64];
    let m = 8 * sub;
    for my in (0..h).step_by(m) {
        for mx in (0..w).step_by(m) {
            for by in 0..sub {
                for bx in 0..sub {
                    for y in 0..8 {
                        for x in 0..8 { f[y * 8 + x] = px(mx + bx * 8 + x, my + by * 8 + y)[0] - 128; }
                    }
                    fdct(&f, &lq, &mut q);
                    encode_block(&mut bw, &q, &mut preds[0], &dcl, &acl);
                }
            }
            if grey { continue; }
            for c in 1..3 {
                // Chroma averaged over sub × sub pixels.
                for y in 0..8 {
                    for x in 0..8 {
                        let mut s = 0;
                        for dy in 0..sub {
                            for dx in 0..sub { s += px(mx + x * sub + dx, my + y * sub + dy)[c]; }
                        }
                        f[y * 8 + x] = (s + (sub * sub / 2) as i32) / (sub * sub) as i32 - 128;
                    }
                }
                fdct(&f, &cq, &mut q);
                encode_block(&mut bw, &q, &mut preds[c], &dcc, &acc);
            }
        }
    }
    bw.flush();
    let mut o = bw.out;
    o.extend_from_slice(&[0xFF, 0xD9]);
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    #[test]
    fn tables() {
        assert_eq!(&ZIGZAG[..10], &[0, 1, 8, 16, 9, 2, 3, 10, 17, 24]);
        assert_eq!(ZIGZAG[63], 63);
        for (lead, bits) in [(AC_LUMA_LEAD, AC_LUMA_BITS), (AC_CHROMA_LEAD, AC_CHROMA_BITS)] {
            let v = ac_vals(lead);
            assert_eq!(v.len(), 162);
            assert_eq!(bits.iter().map(|&b| b as usize).sum::<usize>(), 162);
        }
        // The IDCT of a lone DC coefficient is flat.
        let mut c = [0i32; 64];
        c[0] = 80;
        let mut o = [0u8; 64];
        idct(&c, &mut o);
        assert!(o.iter().all(|&v| v == 138));
    }

    /// A photo-like test image: smooth gradients and a soft disc.
    fn smooth(w: u32, h: u32) -> Image {
        let mut i = Image::new(w, h, 24);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as i32 - w as i32 / 2, y as i32 - h as i32 / 2);
                let d = ((dx * dx + dy * dy) as u32).min(255 * 4) / 4;
                i.set_rgb(x, y, [(x * 255 / w) as u8, (y * 255 / h) as u8, (255 - d) as u8]);
            }
        }
        i
    }

    #[test]
    fn round_trips() {
        let i = smooth(53, 37);
        for q in [50, 75, 95] {
            let e = encode(&i, q);
            assert_eq!(crate::identify(&e, e.len() as u64), Some(crate::Format::Jpeg));
            let d = decode(&e).unwrap();
            let (m, mean) = max_diff(&i, &d);
            assert!(mean < if q > 90 { 3.0 } else { 8.0 }, "q {q}: max {m} mean {mean}");
        }
        // Flat colours come back nearly exact.
        let mut f = Image::new(16, 16, 24);
        for y in 0..16 { for x in 0..16 { f.set_rgb(x, y, [200, 30, 90]); } }
        let (m, _) = max_diff(&f, &decode(&encode(&f, 90)).unwrap());
        assert!(m <= 3, "{m}");
        // Grey palette images stay grey.
        let mut g = Image::new(20, 10, 8);
        g.palette = (0..=255u8).map(|v| [v; 3]).collect();
        for y in 0..10 { for x in 0..20 { g.set_index(x, y, (x * 12) as u8); } }
        let d = decode(&encode(&g, 90)).unwrap();
        assert_eq!(d.bits, 8);
        assert!(max_diff(&g, &d).1 < 3.0);
    }

    #[test]
    fn truncated() {
        let e = encode(&pattern24(24, 24), 75);
        for n in 0..e.len() { let _ = decode(&e[..n]); }
        let mut g = e.clone();
        for k in (2..g.len()).step_by(3) { g[k] ^= 0x33; let _ = decode(&g); }
    }

    /// Converts `src` with sips to a JPEG variant and to BMP and compares.
    fn compare_with_sips(src: &std::path::Path, name: &str) {
        let dir = scratch();
        let bmp = dir.join(format!("{name}.bmp"));
        if !sips(src, "bmp", &bmp) { return; }
        let want = crate::bmp::decode(&std::fs::read(&bmp).unwrap()).unwrap();
        let got = decode(&std::fs::read(src).unwrap()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!((got.width, got.height), (want.width, want.height), "{name}");
        let (m, mean) = max_diff(&want, &got);
        eprintln!("{name}: max {m} mean {mean:.2}");
        assert!(mean < 4.0, "{name}: mean {mean}");
    }

    #[test]
    fn ibm_jpegs() {
        let mut files = work_files("unpacked/disk_14/MMTEMP", "jpg");
        files.extend(work_files("unpacked/disk_21/MMTEMP", "jpg"));
        for p in &files {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let d = decode(&std::fs::read(p).unwrap()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert!(d.width > 0);
            compare_with_sips(p, &name);
        }
        for p in work_files("media/boot/java118", "jpg") {
            decode(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e:?}", p.display()));
        }
    }

    /// Runs jpegtran (when installed) with `args` on `src`.
    fn jpegtran(src: &std::path::Path, args: &[&str], out: &std::path::Path) -> bool {
        std::process::Command::new("jpegtran").args(args).arg("-outfile").arg(out).arg(src)
            .stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
    }

    #[test]
    fn progressive_and_restarts() {
        // jpegtran rewrites the same coefficients progressively or with
        // restart markers: the pixels must come out identical.
        let dir = scratch();
        let mut files = work_files("unpacked/disk_14/MMTEMP", "jpg");
        let ours = dir.join("prog-src.jpg");
        std::fs::write(&ours, encode(&smooth(77, 51), 80)).unwrap();
        files.push(ours);
        for p in &files {
            let base = decode(&std::fs::read(p).unwrap()).unwrap();
            for (n, args) in [("prog", &["-progressive"][..]), ("rst", &["-restart", "1"][..]), ("progrst", &["-progressive", "-restart", "2B"][..])] {
                let out = dir.join(format!("{}-{n}.jpg", p.file_stem().unwrap().to_string_lossy()));
                if !jpegtran(p, args, &out) { return; }
                let d = decode(&std::fs::read(&out).unwrap()).unwrap();
                assert!(d == base, "{} {n}", p.display());
            }
        }
    }

    #[test]
    fn sips_progressive_and_ours() {
        // Our encoder's output read by sips, and sips's JPEG read by us.
        let dir = scratch();
        let src = dir.join("pattern.bmp");
        let i = smooth(100, 75);
        std::fs::write(&src, crate::bmp::encode(&i, crate::bmp::Kind::Win3)).unwrap();
        let j = dir.join("sips.jpg");
        if !sips(&src, "jpeg", &j) { return; }
        compare_with_sips(&j, "sips");
        let ours = dir.join("ours.jpg");
        std::fs::write(&ours, encode(&i, 85)).unwrap();
        let back = dir.join("ours.bmp");
        assert!(sips(&ours, "bmp", &back));
        let d = crate::bmp::decode(&std::fs::read(&back).unwrap()).unwrap();
        let (_, mean) = max_diff(&i, &d);
        assert!(mean < 8.0, "{mean}");
    }
}
