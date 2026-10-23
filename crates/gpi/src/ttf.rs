//! Outline fonts: a TrueType reader (the tables a rasteriser needs:
//! head, maxp, hhea, hmtx, loca, glyf, cmap, OS/2) and an anti-aliased
//! coverage rasteriser (signed area accumulated per pel, as in font-rs),
//! which Type 1 outlines (`type1.rs`) share. No hinting: the scale is
//! nudged so the x-height lands on whole pels, and the baseline is on a
//! pel boundary.

use alloc::vec::Vec;

use crate::math::{abs, ceil, floor, sqrt};

pub type Pf = (f64, f64);

// ---- The coverage rasteriser ----

/// A pel grid accumulating the signed area of closed outlines; y grows
/// downwards (row 0 is the top).
pub struct Coverage {
    pub w: usize,
    pub h: usize,
    acc: Vec<f32>,
}

impl Coverage {
    pub fn new(w: usize, h: usize) -> Coverage {
        Coverage { w, h, acc: alloc::vec![0.0; w * h + 4] }
    }

    /// One edge of an outline (pel coordinates).
    pub fn line(&mut self, p0: Pf, p1: Pf) {
        if abs(p0.1 - p1.1) <= 1e-9 {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 { (1.0f64, p0, p1) } else { (-1.0, p1, p0) };
        let clampx = |x: f64| x.clamp(0.0, self.w as f64 - 1e-6);
        let (p0, p1) = ((clampx(p0.0), p0.1), (clampx(p1.0), p1.1));
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        if p0.1 < 0.0 {
            x -= p0.1 * dxdy;
        }
        let y0 = if p0.1 < 0.0 { 0 } else { p0.1 as usize };
        let y1 = (ceil(p1.1).max(0.0) as usize).min(self.h);
        for y in y0..y1 {
            let start = y * self.w;
            let dy = ((y + 1) as f64).min(p1.1) - (y as f64).max(p0.1);
            let xnext = x + dxdy * dy;
            let d = (dy * dir) as f32;
            let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let xa_floor = floor(xa);
            let xai = xa_floor as usize;
            let xbi = ceil(xb) as usize;
            let a = &mut self.acc;
            if xbi <= xai + 1 {
                let xmf = (0.5 * (x + xnext) - xa_floor) as f32;
                a[start + xai] += d - d * xmf;
                a[start + xai + 1] += d * xmf;
            } else {
                let s = (1.0 / (xb - xa)) as f32;
                let xaf = (xa - xa_floor) as f32;
                let a0 = 0.5 * s * (1.0 - xaf) * (1.0 - xaf);
                let xbf = (xb - ceil(xb) + 1.0) as f32;
                let am = 0.5 * s * xbf * xbf;
                a[start + xai] += d * a0;
                if xbi == xai + 2 {
                    a[start + xai + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - xaf);
                    a[start + xai + 1] += d * (a1 - a0);
                    for xi in xai + 2..xbi - 1 {
                        a[start + xi] += d * s;
                    }
                    let a2 = a1 + (xbi - xai - 3) as f32 * s;
                    a[start + xbi - 1] += d * (1.0 - a2 - am);
                }
                a[start + xbi] += d * am;
            }
            x = xnext;
        }
    }

    /// A quadratic Bézier, flattened.
    pub fn quad(&mut self, p0: Pf, p1: Pf, p2: Pf) {
        let dd = abs(p0.0 - 2.0 * p1.0 + p2.0) + abs(p0.1 - 2.0 * p1.1 + p2.1);
        let n = (sqrt(dd * 2.0) as usize).clamp(1, 32);
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let p = (u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1);
            self.line(prev, p);
            prev = p;
        }
    }

    /// A cubic Bézier, flattened.
    pub fn cubic(&mut self, p0: Pf, p1: Pf, p2: Pf, p3: Pf) {
        let dd = abs(p0.0 - 2.0 * p1.0 + p2.0) + abs(p0.1 - 2.0 * p1.1 + p2.1) + abs(p1.0 - 2.0 * p2.0 + p3.0) + abs(p1.1 - 2.0 * p2.1 + p3.1);
        let n = (sqrt(dd * 3.0) as usize).clamp(1, 48);
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            let p = (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1);
            self.line(prev, p);
            prev = p;
        }
    }

    /// The coverage of each pel, 0..=255, through `GAMMA` (text a little
    /// darker, as eyes expect of small type).
    pub fn finish(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h);
        let mut sum = 0.0f32;
        for i in 0..self.w * self.h {
            sum += self.acc[i];
            let v = if sum < 0.0 { -sum } else { sum };
            out.push(gamma((v.min(1.0) * 255.0 + 0.5) as u8));
        }
        out
    }
}

/// Coverage to opacity: c^0.8, so thin stems keep their weight.
fn gamma(c: u8) -> u8 {
    static mut LUT: Option<[u8; 256]> = None;
    let lut = unsafe { &mut *core::ptr::addr_of_mut!(LUT) }.get_or_insert_with(|| {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let x = i as f64 / 255.0;
            // x^0.8 = x / x^0.2; x^0.2 by Newton's method on y^5 = x.
            let r = if x <= 0.0 {
                0.0
            } else {
                let mut y = 1.0f64;
                for _ in 0..30 {
                    y -= (y * y * y * y * y - x) / (5.0 * y * y * y * y);
                }
                x / y
            };
            *v = (r * 255.0 + 0.5).min(255.0) as u8;
        }
        t
    });
    lut[c as usize]
}

/// A glyph rendered at some size: `cov` is `w` x `h` pels; the bitmap's
/// left edge is `left` pels right of the pen and its top row `top` pels
/// above the baseline; `advance` is the pen's move.
#[derive(Clone, Default)]
pub struct Rendered {
    pub advance: i32,
    pub left: i32,
    pub top: i32,
    pub w: i32,
    pub h: i32,
    pub cov: Vec<u8>,
}

/// An outline as path operations (font units, y up): move, line,
/// quadratic and cubic curves; each figure closes at the next move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    M(Pf),
    L(Pf),
    Q(Pf, Pf),
    C(Pf, Pf, Pf),
}

pub type Outline = Vec<Op>;

fn op_points(o: &Op) -> &[Pf] {
    match o {
        Op::M(p) | Op::L(p) => core::slice::from_ref(p),
        Op::Q(a, _) => core::slice::from_ref(a),
        Op::C(a, _, _) => core::slice::from_ref(a),
    }
}

/// Render an outline (font units, y up) at `scale` pels per unit;
/// `bold` > 0 emboldens by that many pels.
pub fn render(outline: &Outline, scale: f64, advance: i32, bold: f64) -> Rendered {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut see = |&(x, y): &Pf| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    for o in outline {
        op_points(o).iter().for_each(&mut see);
        match o {
            Op::Q(_, b) => see(b),
            Op::C(_, b, c) => {
                see(b);
                see(c);
            }
            _ => {}
        }
    }
    if x0 > x1 {
        return Rendered { advance, ..Default::default() };
    }
    let left = floor(x0 * scale) as i32 - 1;
    let top = ceil(y1 * scale) as i32 + 1;
    let w = (ceil(x1 * scale + bold) as i32 + 2 - left).max(1);
    let h = (top - floor(y0 * scale) as i32 + 2).max(1);
    let mut cov = Coverage::new(w as usize, h as usize);
    // Emboldening: the outline drawn again a little to the right (the
    // coverage clamps at 1).
    let passes: &[f64] = if bold > 0.0 { &[0.0, 0.5, 1.0] } else { &[0.0] };
    for &k in passes {
        let dx = k * bold;
        let map = |p: Pf| (p.0 * scale - left as f64 + dx, top as f64 - p.1 * scale);
        let (mut start, mut cur) = ((0.0, 0.0), (0.0, 0.0));
        let mut open = false;
        for o in outline {
            match *o {
                Op::M(p) => {
                    if open && cur != start {
                        cov.line(cur, start);
                    }
                    start = map(p);
                    cur = start;
                    open = true;
                }
                Op::L(p) => {
                    let p = map(p);
                    cov.line(cur, p);
                    cur = p;
                }
                Op::Q(c, p) => {
                    let p = map(p);
                    cov.quad(cur, map(c), p);
                    cur = p;
                }
                Op::C(a, b, p) => {
                    let p = map(p);
                    cov.cubic(cur, map(a), map(b), p);
                    cur = p;
                }
            }
        }
        if open && cur != start {
            cov.line(cur, start);
        }
    }
    Rendered { advance, left, top, w, h, cov: cov.finish() }
}

/// A TrueType contour as path operations: off-curve points are
/// quadratic controls, two in a row imply the on-curve point between.
fn quad_contour(c: &[(Pf, bool)], out: &mut Outline) {
    let n = c.len();
    if n < 2 {
        return;
    }
    let mid = |a: Pf, b: Pf| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let s = (0..n).find(|&i| c[i].1);
    let start = match s {
        Some(i) => c[i].0,
        None => mid(c[0].0, c[n - 1].0),
    };
    // With no on-curve point, walk all n points from the implied start.
    let (s, count) = match s {
        Some(i) => (i, n - 1),
        None => (n - 1, n),
    };
    out.push(Op::M(start));
    let mut ctrl: Option<Pf> = None;
    for k in 1..=count {
        let (p, on) = c[(s + k) % n];
        match (on, ctrl) {
            (true, None) => out.push(Op::L(p)),
            (true, Some(q)) => {
                out.push(Op::Q(q, p));
                ctrl = None;
            }
            (false, None) => ctrl = Some(p),
            (false, Some(q)) => {
                out.push(Op::Q(q, mid(q, p)));
                ctrl = Some(p);
            }
        }
    }
    match ctrl {
        Some(q) => out.push(Op::Q(q, start)),
        None => out.push(Op::L(start)),
    }
}

// ---- TrueType ----

/// A TrueType font file.
pub struct Ttf {
    data: &'static [u8],
    pub units: u16,
    pub glyphs: u16,
    long_loca: bool,
    loca: usize,
    glyf: usize,
    hmtx: usize,
    hmetrics: u16,
    cmap: usize,
    cmap_format: u16,
    /// Typographic metrics (font units): ascender, descender (positive
    /// below), the OS/2 x-height and cap height (0 when absent), the
    /// average character width.
    pub ascender: i32,
    pub descender: i32,
    pub x_height: i32,
    pub cap_height: i32,
    pub avg_width: i32,
    /// The family and full names (name table ids 1 and 4).
    pub family: Vec<u8>,
    pub full_name: Vec<u8>,
    pub bold: bool,
    pub italic: bool,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(o)?, *b.get(o + 1)?]))
}
fn i16_at(b: &[u8], o: usize) -> Option<i16> {
    u16_at(b, o).map(|v| v as i16)
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?]))
}

impl Ttf {
    pub fn parse(data: &'static [u8]) -> Option<Ttf> {
        let ver = u32_at(data, 0)?;
        if ver != 0x0001_0000 && ver != 0x7472_7565 {
            return None;
        }
        let n = u16_at(data, 4)? as usize;
        let table = |tag: &[u8; 4]| -> Option<usize> {
            (0..n).map(|i| 12 + 16 * i).find(|&o| data.get(o..o + 4) == Some(&tag[..])).and_then(|o| u32_at(data, o + 8)).map(|v| v as usize)
        };
        let head = table(b"head")?;
        let maxp = table(b"maxp")?;
        let hhea = table(b"hhea")?;
        let os2 = table(b"OS/2");
        let cmap = table(b"cmap")?;
        // cmap: Windows Unicode full (3, 10, format 12), else BMP (3, 1,
        // format 4), else Unicode (0, *).
        let nsub = u16_at(data, cmap + 2)? as usize;
        let mut best: Option<(u32, usize)> = None;
        for i in 0..nsub {
            let o = cmap + 4 + 8 * i;
            let (pid, eid) = (u16_at(data, o)?, u16_at(data, o + 2)?);
            let off = cmap + u32_at(data, o + 4)? as usize;
            let fmt = u16_at(data, off)?;
            let rank = match (pid, eid, fmt) {
                (3, 10, 12) => 4,
                (0, _, 12) => 3,
                (3, 1, 4) => 2,
                (0, _, 4) => 1,
                _ => 0,
            };
            if rank > 0 && best.is_none_or(|(r, _)| rank > r) {
                best = Some((rank, off));
            }
        }
        let (_, sub) = best?;
        let (ascender, descender) = (i16_at(data, hhea + 4)? as i32, -(i16_at(data, hhea + 6)? as i32));
        let (mut x_height, mut cap_height, mut avg_width, mut bold, mut italic) = (0, 0, 0, false, false);
        if let Some(o) = os2 {
            avg_width = i16_at(data, o + 2)? as i32;
            let sel = u16_at(data, o + 62)?;
            italic = sel & 1 != 0;
            bold = sel & 0x20 != 0;
            if u16_at(data, o)? >= 2 {
                x_height = i16_at(data, o + 86).unwrap_or(0) as i32;
                cap_height = i16_at(data, o + 88).unwrap_or(0) as i32;
            }
        }
        let mut t = Ttf {
            data,
            units: u16_at(data, head + 18)?,
            glyphs: u16_at(data, maxp + 4)?,
            long_loca: i16_at(data, head + 50)? != 0,
            loca: table(b"loca")?,
            glyf: table(b"glyf")?,
            hmtx: table(b"hmtx")?,
            hmetrics: u16_at(data, hhea + 34)?,
            cmap: sub,
            cmap_format: u16_at(data, sub)?,
            ascender,
            descender,
            x_height,
            cap_height,
            avg_width,
            family: Vec::new(),
            full_name: Vec::new(),
            bold,
            italic,
        };
        if let Some(name) = table(b"name") {
            t.family = t.name(name, 1).unwrap_or_default();
            t.full_name = t.name(name, 4).unwrap_or_default();
        }
        if t.avg_width == 0 {
            t.avg_width = t.advance(t.glyph_index('x')) as i32;
        }
        Some(t)
    }

    /// A name record, from the Windows (UTF-16, ASCII kept) or Mac
    /// (Roman) platform.
    fn name(&self, table: usize, id: u16) -> Option<Vec<u8>> {
        let d = self.data;
        let n = u16_at(d, table + 2)? as usize;
        let strings = table + u16_at(d, table + 4)? as usize;
        let mut mac = None;
        for i in 0..n {
            let o = table + 6 + 12 * i;
            let (pid, nid) = (u16_at(d, o)?, u16_at(d, o + 6)?);
            if nid != id {
                continue;
            }
            let (len, off) = (u16_at(d, o + 8)? as usize, u16_at(d, o + 10)? as usize);
            let s = d.get(strings + off..strings + off + len)?;
            if pid == 3 || pid == 0 {
                return Some(s.chunks(2).map(|c| if c[0] == 0 { c.get(1).copied().unwrap_or(b'?') } else { b'?' }).collect());
            }
            mac = Some(s.to_vec());
        }
        mac
    }

    /// The glyph of a character (0: .notdef).
    pub fn glyph_index(&self, ch: char) -> u16 {
        let c = ch as u32;
        let d = self.data;
        let s = self.cmap;
        let r = || -> Option<u16> {
            if self.cmap_format == 12 {
                let n = u32_at(d, s + 12)? as usize;
                let (mut lo, mut hi) = (0, n);
                while lo < hi {
                    let m = (lo + hi) / 2;
                    let o = s + 16 + 12 * m;
                    let (a, b) = (u32_at(d, o)?, u32_at(d, o + 4)?);
                    if c < a {
                        hi = m;
                    } else if c > b {
                        lo = m + 1;
                    } else {
                        return Some((u32_at(d, o + 8)? + (c - a)) as u16);
                    }
                }
                return Some(0);
            }
            if c > 0xffff {
                return Some(0);
            }
            let segs = u16_at(d, s + 6)? as usize / 2;
            let ends = s + 14;
            let starts = ends + 2 * segs + 2;
            let deltas = starts + 2 * segs;
            let offsets = deltas + 2 * segs;
            let k = (0..segs).find(|&k| u16_at(d, ends + 2 * k).is_some_and(|e| e as u32 >= c))?;
            let start = u16_at(d, starts + 2 * k)? as u32;
            if c < start {
                return Some(0);
            }
            let delta = u16_at(d, deltas + 2 * k)?;
            let ro = u16_at(d, offsets + 2 * k)? as usize;
            if ro == 0 {
                return Some((c as u16).wrapping_add(delta));
            }
            let g = u16_at(d, offsets + 2 * k + ro + 2 * (c - start) as usize)?;
            Some(if g == 0 { 0 } else { g.wrapping_add(delta) })
        };
        r().unwrap_or(0)
    }

    /// Advance width of a glyph (font units).
    pub fn advance(&self, g: u16) -> u16 {
        let i = (g.min(self.hmetrics.saturating_sub(1))) as usize;
        u16_at(self.data, self.hmtx + 4 * i).unwrap_or(0)
    }

    fn glyph_range(&self, g: u16) -> Option<(usize, usize)> {
        let g = g as usize;
        let (a, b) = if self.long_loca {
            (u32_at(self.data, self.loca + 4 * g)? as usize, u32_at(self.data, self.loca + 4 * g + 4)? as usize)
        } else {
            (u16_at(self.data, self.loca + 2 * g)? as usize * 2, u16_at(self.data, self.loca + 2 * g + 2)? as usize * 2)
        };
        (b > a).then_some((self.glyf + a, self.glyf + b))
    }

    /// A glyph's outline (font units, y up).
    pub fn outline(&self, g: u16) -> Outline {
        let mut out = Vec::new();
        self.add_outline(g, &mut out, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0);
        out
    }

    fn add_outline(&self, g: u16, out: &mut Outline, m: [f64; 6], depth: u32) -> Option<()> {
        if depth > 8 {
            return None;
        }
        let d = self.data;
        let (o, end) = self.glyph_range(g)?;
        let nc = i16_at(d, o)?;
        let tf = |x: f64, y: f64| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        if nc >= 0 {
            let nc = nc as usize;
            let ends: Vec<usize> = (0..nc).map(|i| u16_at(d, o + 10 + 2 * i).unwrap_or(0) as usize).collect();
            let npts = ends.last().map_or(0, |&e| e + 1);
            let ilen = u16_at(d, o + 10 + 2 * nc)? as usize;
            let mut p = o + 12 + 2 * nc + ilen;
            let mut flags = Vec::with_capacity(npts);
            while flags.len() < npts {
                let f = *d.get(p)?;
                p += 1;
                flags.push(f);
                if f & 8 != 0 {
                    let r = *d.get(p)?;
                    p += 1;
                    for _ in 0..r {
                        flags.push(f);
                    }
                }
            }
            flags.truncate(npts);
            let mut xs = Vec::with_capacity(npts);
            let mut v = 0i32;
            for &f in &flags {
                if f & 2 != 0 {
                    let b = *d.get(p)? as i32;
                    p += 1;
                    v += if f & 0x10 != 0 { b } else { -b };
                } else if f & 0x10 == 0 {
                    v += i16_at(d, p)? as i32;
                    p += 2;
                }
                xs.push(v);
            }
            let mut ys = Vec::with_capacity(npts);
            v = 0;
            for &f in &flags {
                if f & 4 != 0 {
                    let b = *d.get(p)? as i32;
                    p += 1;
                    v += if f & 0x20 != 0 { b } else { -b };
                } else if f & 0x20 == 0 {
                    v += i16_at(d, p)? as i32;
                    p += 2;
                }
                ys.push(v);
            }
            if p > end + 4 {
                return None;
            }
            let mut s = 0;
            for &e in &ends {
                let c: Vec<(Pf, bool)> = (s..=e.min(npts.saturating_sub(1))).map(|i| (tf(xs[i] as f64, ys[i] as f64), flags[i] & 1 != 0)).collect();
                quad_contour(&c, out);
                s = e + 1;
            }
        } else {
            // A composite: components with offsets and transforms.
            let mut p = o + 10;
            loop {
                let flags = u16_at(d, p)?;
                let gi = u16_at(d, p + 2)?;
                p += 4;
                let (dx, dy) = if flags & 1 != 0 {
                    let r = (i16_at(d, p)? as f64, i16_at(d, p + 2)? as f64);
                    p += 4;
                    r
                } else {
                    let r = (*d.get(p)? as i8 as f64, *d.get(p + 1)? as i8 as f64);
                    p += 2;
                    r
                };
                let f2 = |o: usize| i16_at(d, o).map(|v| v as f64 / 16384.0);
                let (mut a, mut b, mut c, mut dd) = (1.0, 0.0, 0.0, 1.0);
                if flags & 0x08 != 0 {
                    a = f2(p)?;
                    dd = a;
                    p += 2;
                } else if flags & 0x40 != 0 {
                    a = f2(p)?;
                    dd = f2(p + 2)?;
                    p += 4;
                } else if flags & 0x80 != 0 {
                    a = f2(p)?;
                    b = f2(p + 2)?;
                    c = f2(p + 4)?;
                    dd = f2(p + 6)?;
                    p += 8;
                }
                // ARGS_ARE_XY_VALUES (point matching is not supported).
                let (ox, oy) = if flags & 2 != 0 { (dx, dy) } else { (0.0, 0.0) };
                let cm = [a, b, c, dd, ox, oy];
                let comb = [
                    m[0] * cm[0] + m[2] * cm[1],
                    m[1] * cm[0] + m[3] * cm[1],
                    m[0] * cm[2] + m[2] * cm[3],
                    m[1] * cm[2] + m[3] * cm[3],
                    m[0] * cm[4] + m[2] * cm[5] + m[4],
                    m[1] * cm[4] + m[3] * cm[5] + m[5],
                ];
                self.add_outline(gi, out, comb, depth + 1);
                if flags & 0x20 == 0 {
                    break;
                }
            }
        }
        Some(())
    }

    /// The pels per font unit for a font `px` pels high (em): nudged so
    /// the x-height is a whole number of pels.
    pub fn scale_for(&self, px: f64) -> f64 {
        let s = px / self.units as f64;
        if self.x_height > 0 && px < 40.0 {
            let xh = self.x_height as f64 * s;
            let snapped = floor(xh + 0.5).max(1.0);
            let k = snapped / xh;
            if (0.92..=1.08).contains(&k) {
                return s * k;
            }
        }
        s
    }

    /// Render character `ch` at `scale` pels per unit.
    pub fn render(&self, ch: char, scale: f64, bold: f64) -> Rendered {
        let g = self.glyph_index(ch);
        let adv = floor(self.advance(g) as f64 * scale + bold + 0.5) as i32;
        render(&self.outline(g), scale, adv, bold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_covers() {
        let mut c = Coverage::new(4, 4);
        let sq = [(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0)];
        for i in 0..4 {
            c.line(sq[i], sq[(i + 1) % 4]);
        }
        let v = c.finish();
        assert_eq!(v[5], 255);
        assert_eq!(v[0], 0);
        assert_eq!(v[15], 0);
    }

    #[test]
    fn half_pel_edge() {
        let mut c = Coverage::new(4, 2);
        let r = [(0.5, 0.0), (2.0, 0.0), (2.0, 2.0), (0.5, 2.0)];
        for i in 0..4 {
            c.line(r[i], r[(i + 1) % 4]);
        }
        let v = c.finish();
        assert!(v[0] > 100 && v[0] < 255, "{}", v[0]);
        assert_eq!(v[1], 255);
        assert_eq!(v[2], 0);
    }

    #[test]
    fn liberation_sans() {
        extern crate std;
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dist/fonts/LIBSANS.TTF");
        let Ok(data) = std::fs::read(p) else { return };
        let data: &'static [u8] = std::boxed::Box::leak(data.into_boxed_slice());
        let t = Ttf::parse(data).expect("parses");
        assert_eq!(t.units, 2048);
        assert_eq!(t.family, b"Liberation Sans");
        let g = t.glyph_index('A');
        assert!(g != 0);
        assert_ne!(t.glyph_index('é'), 0);
        let s = t.scale_for(13.0);
        let r = t.render('H', s, 0.0);
        assert!(r.advance >= 8 && r.advance <= 11, "advance {}", r.advance);
        assert!(r.cov.iter().filter(|&&c| c > 200).count() > 10);
        // Composite (Aacute) renders too.
        let r = t.render('Á', s, 0.0);
        assert!(r.cov.iter().any(|&c| c > 200));
    }
}

#[cfg(test)]
mod dump {
    #[test]
    #[ignore]
    fn show() {
        extern crate std;
        use std::println;
        let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dist/fonts/LIBSANS.TTF")).unwrap();
        let data: &'static [u8] = std::boxed::Box::leak(data.into_boxed_slice());
        let t = super::Ttf::parse(data).unwrap();
        for ch in ['H', 'g', 'e'] {
            let r = t.render(ch, t.scale_for(13.0), 0.0);
            println!("{ch} adv {} left {} top {} {}x{} outline {:?}", r.advance, r.left, r.top, r.w, r.h, &t.outline(t.glyph_index(ch))[..3.min(t.outline(t.glyph_index(ch)).len())]);
            for y in 0..r.h {
                let row: std::string::String = (0..r.w).map(|x| match r.cov[(y * r.w + x) as usize] { 0 => '.', 1..=100 => '-', 101..=200 => '+', _ => '#' }).collect();
                println!("{row}");
            }
        }
    }
}
