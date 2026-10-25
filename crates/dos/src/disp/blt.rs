//! BITBLT and REALIZEOBJECT: rows of pixels of the screen and of memory
//! bitmaps, the 256 raster operations, brushes and pens, and the
//! conversions between colour and monochrome.

use alloc::vec;
use alloc::vec::Vec;

use super::{mono_of, nearest, phys, Args, Dev, DrawMode, Ptr, Rect};
use crate::Machine;

/// A realized brush (REALIZEOBJECT, 80 bytes in GDI's memory).
pub(crate) const BRUSH_SIZE: u32 = 80;
pub(crate) const PEN_SIZE: u32 = 16;
const BRUSH_MAGIC: u16 = 0x4252;
const PEN_MAGIC: u16 = 0x5052;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BrushKind {
    /// The colour pattern as it is.
    Pattern,
    Hollow,
    /// The mono pattern's 1s in the brush colour, 0s in the background
    /// colour (or left alone when the background mode is transparent).
    Hatched,
    /// A monochrome pattern bitmap: 1s in the background colour, 0s in
    /// the text colour.
    Mono,
}

#[derive(Clone, Debug)]
pub(crate) struct Brush {
    pub kind: BrushKind,
    pub fg: u8,
    pub fg_mono: u8,
    pub colour: [u8; 64],
    pub mono: [u8; 8],
}

impl Brush {
    pub fn solid(p: u32) -> Brush {
        let m = if p & 0x100 != 0 { 0xff } else { 0 };
        Brush { kind: BrushKind::Pattern, fg: p as u8, fg_mono: (p >> 8) as u8 & 1, colour: [p as u8; 64], mono: [m; 8] }
    }

    /// Row `y` (0-7) of the pattern for a destination, as pixel values
    /// (indices, or 0/1 for monochrome), and which pixels it leaves alone
    /// (transparent hatch backgrounds).
    pub fn row(&self, y: usize, mono: bool, dm: &DrawMode, transparent_ok: bool) -> ([u8; 8], u8) {
        let mut out = [0u8; 8];
        let bits = self.mono[y & 7];
        let mut keep = 0u8;
        for (x, o) in out.iter_mut().enumerate() {
            let bit = bits >> (7 - x) & 1;
            *o = match self.kind {
                BrushKind::Pattern | BrushKind::Hollow => {
                    if mono {
                        bit
                    } else {
                        self.colour[(y & 7) * 8 + x]
                    }
                }
                BrushKind::Hatched => {
                    if bit == 0 && transparent_ok && dm.bk_mode == 1 {
                        keep |= 0x80 >> x;
                    }
                    match (bit, mono) {
                        (1, true) => self.fg_mono,
                        (1, false) => self.fg,
                        (_, true) => (dm.bk >> 8) as u8 & 1,
                        _ => dm.bk as u8,
                    }
                }
                BrushKind::Mono => match (bit, mono) {
                    (1, true) => (dm.bk >> 8) as u8 & 1,
                    (1, false) => dm.bk as u8,
                    (_, true) => (dm.text >> 8) as u8 & 1,
                    _ => dm.text as u8,
                },
            };
        }
        (out, keep)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pen {
    /// PS_SOLID 0, DASH 1, DOT 2, DASHDOT 3, DASHDOTDOT 4, NULL 5.
    pub style: u8,
    pub phys: u32,
}

/// Hatch patterns (HS_HORIZONTAL .. HS_DIAGCROSS), 1 bits are lines.
const HATCHES: [[u8; 8]; 6] = [
    [0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00],
    [0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08],
    [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01],
    [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80],
    [0x08, 0x08, 0x08, 0x08, 0xff, 0x08, 0x08, 0x08],
    [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81],
];

/// One raster operation on pixel values: bit by bit, `rop` the ROP3
/// index (bit P*4 + S*2 + D of it is the result).
#[inline]
pub(crate) fn rop3(rop: u8, p: u8, s: u8, d: u8) -> u8 {
    match rop {
        0xcc => s,
        0xf0 => p,
        0x00 => 0,
        0xff => 0xff,
        0x55 => !d,
        0x5a => p ^ d,
        0x66 => s ^ d,
        0x88 => s & d,
        0xee => s | d,
        0x33 => !s,
        0xbb => !s | d,
        0x44 => s & !d,
        0x11 => !(s | d),
        0xc0 => p & s,
        0xaa => d,
        _ => {
            let mut r = 0u8;
            for k in 0..8u8 {
                if rop >> k & 1 != 0 {
                    let pm = if k & 4 != 0 { p } else { !p };
                    let sm = if k & 2 != 0 { s } else { !s };
                    let dm = if k & 1 != 0 { d } else { !d };
                    r |= pm & sm & dm;
                }
            }
            r
        }
    }
}

/// A binary raster operation (R2_BLACK 1 .. R2_WHITE 16) on pen P and
/// destination D.
#[inline]
pub(crate) fn rop2(r: u8, p: u8, d: u8) -> u8 {
    match r {
        13 => p,
        7 => p ^ d,
        6 => !d,
        11 => d,
        1 => 0,
        16 => 0xff,
        _ => {
            let t = r.wrapping_sub(1);
            let mut v = 0u8;
            for k in 0..4u8 {
                if t >> k & 1 != 0 {
                    let pm = if k & 2 != 0 { p } else { !p };
                    let dm = if k & 1 != 0 { d } else { !d };
                    v |= pm & dm;
                }
            }
            v
        }
    }
}

impl Machine {
    /// Pixels x0..x0+n of row y as values (0/1 for monochrome).
    pub(crate) fn dev_read(&mut self, d: &Dev, y: i32, x0: i32, out: &mut [u8]) {
        let n = out.len();
        if d.screen {
            let o = y as usize * self.disp.w + x0 as usize;
            out.copy_from_slice(&self.disp.pix[o..o + n]);
            return;
        }
        let l = d.row_lin(y);
        if d.mono {
            let b0 = x0 as usize / 8;
            let b1 = (x0 as usize + n).div_ceil(8);
            let mut bytes = vec![0u8; b1 - b0];
            self.hw.read_bytes(l + b0 as u32, &mut bytes);
            for (i, o) in out.iter_mut().enumerate() {
                let x = x0 as usize + i;
                *o = bytes[x / 8 - b0] >> (7 - x % 8) & 1;
            }
        } else {
            self.hw.read_bytes(l + x0 as u32, out);
        }
    }

    /// Write pixel values to row y from x0, leaving pixels whose bit in
    /// `keep` (by position, MSB first, from x0) is set.
    pub(crate) fn dev_write(&mut self, d: &Dev, y: i32, x0: i32, px: &[u8]) {
        let n = px.len();
        if n == 0 {
            return;
        }
        if d.screen {
            let o = y as usize * self.disp.w + x0 as usize;
            self.disp.pix[o..o + n].copy_from_slice(px);
            return;
        }
        let l = d.row_lin(y);
        if d.mono {
            let b0 = x0 as usize / 8;
            let b1 = (x0 as usize + n).div_ceil(8);
            let mut bytes = vec![0u8; b1 - b0];
            self.hw.read_bytes(l + b0 as u32, &mut bytes);
            for (i, &v) in px.iter().enumerate() {
                let x = x0 as usize + i;
                let m = 0x80 >> (x % 8);
                if v & 1 != 0 {
                    bytes[x / 8 - b0] |= m;
                } else {
                    bytes[x / 8 - b0] &= !m;
                }
            }
            self.hw.write_bytes(l + b0 as u32, &bytes);
        } else {
            self.hw.write_bytes(l + x0 as u32, px);
        }
    }

    /// The realized brush at `p` (None: NULL or hollow).
    pub(crate) fn disp_brush(&mut self, p: Ptr) -> Option<Brush> {
        if p.null() {
            return None;
        }
        let l = self.far(p);
        if self.hw.rd16(l) != BRUSH_MAGIC {
            return None;
        }
        let kind = match self.hw.rd(l + 2) {
            0 => BrushKind::Pattern,
            1 => return None,
            2 => BrushKind::Hatched,
            _ => BrushKind::Mono,
        };
        let mut colour = [0u8; 64];
        let mut mono = [0u8; 8];
        self.hw.read_bytes(l + 8, &mut colour);
        self.hw.read_bytes(l + 72, &mut mono);
        Some(Brush { kind, fg: self.hw.rd(l + 3), fg_mono: self.hw.rd(l + 4), colour, mono })
    }

    pub(crate) fn disp_pen(&mut self, p: Ptr) -> Option<Pen> {
        if p.null() {
            return None;
        }
        let l = self.far(p);
        if self.hw.rd16(l) != PEN_MAGIC {
            return None;
        }
        let style = self.hw.rd(l + 2);
        if style == 5 {
            return None;
        }
        Some(Pen { style, phys: self.hw.rd32(l + 4) })
    }

    /// REALIZEOBJECT(lpDestDev, wStyle, lpInObj, lpOutObj, lpTextXForm):
    /// pens (1) and brushes (2); no device fonts (3).
    pub(crate) fn disp_realize(&mut self, a: &mut Args) -> u32 {
        let _dev = a.p();
        let style = a.w() as i16;
        let pin = a.p();
        let pout = a.p();
        let _xform = a.p();
        if style < 0 {
            return 1;
        }
        if pout.null() {
            return match style {
                1 => PEN_SIZE,
                2 => BRUSH_SIZE,
                _ => 0,
            };
        }
        let li = self.far(pin);
        let lo = self.far(pout);
        match style {
            1 => {
                // LOGPEN: style, width (POINT), colour.
                let ps = self.hw.rd16(li) as u8;
                let c = self.hw.rd32(li + 6);
                let p = phys(c);
                let mut b = [0u8; PEN_SIZE as usize];
                b[0..2].copy_from_slice(&PEN_MAGIC.to_le_bytes());
                b[2] = ps.min(6);
                b[4..8].copy_from_slice(&p.to_le_bytes());
                self.hw.write_bytes(lo, &b);
                PEN_SIZE
            }
            2 => {
                // LOGBRUSH: style, colour, hatch.
                let bs = self.hw.rd16(li);
                let c = self.hw.rd32(li + 2);
                let hatch = self.hw.rd16(li + 6) as usize;
                let br = match bs {
                    0 => Some(Brush::solid(phys(c))),
                    1 => None,
                    2 => {
                        let p = phys(c);
                        let mut b = Brush::solid(p);
                        b.kind = BrushKind::Hatched;
                        b.mono = HATCHES[hatch.min(5)];
                        Some(b)
                    }
                    3 => self.pattern_brush(Ptr { off: c as u16, sel: (c >> 16) as u16 }),
                    _ => Some(Brush::solid(phys(c))),
                };
                let mut b = [0u8; BRUSH_SIZE as usize];
                b[0..2].copy_from_slice(&BRUSH_MAGIC.to_le_bytes());
                match br {
                    None => b[2] = 1,
                    Some(br) => {
                        b[2] = match br.kind {
                            BrushKind::Pattern => 0,
                            BrushKind::Hollow => 1,
                            BrushKind::Hatched => 2,
                            BrushKind::Mono => 3,
                        };
                        b[3] = br.fg;
                        b[4] = br.fg_mono;
                        b[8..72].copy_from_slice(&br.colour);
                        b[72..80].copy_from_slice(&br.mono);
                    }
                }
                self.hw.write_bytes(lo, &b);
                BRUSH_SIZE
            }
            _ => 0,
        }
    }

    /// A pattern brush from a BITMAP (8x8 used).
    fn pattern_brush(&mut self, bm: Ptr) -> Option<Brush> {
        let d = self.disp_dev(bm)?;
        let mut b = Brush::solid(0);
        let mut row = [0u8; 8];
        let (w, h) = (d.w.min(8), d.h.min(8));
        for y in 0..8 {
            if w <= 0 || h <= 0 {
                break;
            }
            for x in 0..8 {
                let mut px = [0u8; 1];
                self.dev_read(&d, y % h, x % w, &mut px);
                row[x as usize] = px[0];
            }
            let mut m = 0u8;
            for x in 0..8 {
                let v = row[x];
                let bit = if d.mono { v & 1 } else { (super::PALETTE[v as usize] != 0 && mono_of(super::cref_of(v)) == 1) as u8 };
                m |= bit << (7 - x);
                b.colour[y as usize * 8 + x] = if d.mono { if v & 1 != 0 { 255 } else { 0 } } else { v };
            }
            b.mono[y as usize] = m;
        }
        if d.mono {
            b.kind = BrushKind::Mono;
        }
        Some(b)
    }

    /// BITBLT(lpDestDev, DestX, DestY, lpSrcDev, SrcX, SrcY, XExt, YExt,
    /// Rop3, lpPBrush, lpDrawMode). GDI has clipped the rectangle.
    pub(crate) fn disp_bitblt(&mut self, a: &mut Args) -> u32 {
        let pd = a.p();
        let dx = a.i();
        let dy = a.i();
        let ps = a.p();
        let sx = a.i();
        let sy = a.i();
        let w = a.i();
        let h = a.i();
        let rop = (a.d() >> 16) as u8;
        let pb = a.p();
        let pdm = a.p();
        let Some(dst) = self.disp_dev(pd) else { return 0 };
        let dm = self.disp_drawmode(pdm);
        let src = if ps.null() { None } else { self.disp_dev(ps) };
        let brush = self.disp_brush(pb);
        self.blt(&dst, dx, dy, src.as_ref(), sx, sy, w, h, rop, brush.as_ref(), &dm);
        1
    }

    /// The raster operation `rop` on a w x h rectangle at (dx, dy) of
    /// `dst`, with the source at (sx, sy) of `src`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn blt(&mut self, dst: &Dev, mut dx: i32, mut dy: i32, src: Option<&Dev>, mut sx: i32, mut sy: i32, mut w: i32, mut h: i32, rop: u8, brush: Option<&Brush>, dm: &DrawMode) {
        let uses_s = ((rop >> 2) ^ rop) & 0x33 != 0;
        let uses_p = ((rop >> 4) ^ rop) & 0x0f != 0;
        let uses_d = ((rop >> 1) ^ rop) & 0x55 != 0;
        let src = if uses_s { src } else { None };
        if uses_s && src.is_none() {
            return;
        }
        // Clip to the destination and the source.
        let clip = |d0: &mut i32, s0: &mut i32, len: &mut i32, dlim: i32, slim: Option<i32>| {
            if *d0 < 0 {
                *len += *d0;
                *s0 -= *d0;
                *d0 = 0;
            }
            if *d0 + *len > dlim {
                *len = dlim - *d0;
            }
            if let Some(sl) = slim {
                if *s0 < 0 {
                    *len += *s0;
                    *d0 -= *s0;
                    *s0 = 0;
                }
                if *s0 + *len > sl {
                    *len = sl - *s0;
                }
            }
        };
        clip(&mut dx, &mut sx, &mut w, dst.w, src.map(|s| s.w));
        clip(&mut dy, &mut sy, &mut h, dst.h, src.map(|s| s.h));
        if w <= 0 || h <= 0 {
            return;
        }
        let n = w as usize;
        // Rows of the source first when it is the destination too (bottom
        // up when moving down).
        let same = src.is_some_and(|s| s.screen && dst.screen) || src.is_some_and(|s| !s.screen && !dst.screen && s.segs == dst.segs);
        let order: Vec<i32> = if same && dy > sy { (0..h).rev().collect() } else { (0..h).collect() };
        let (bk, text) = if dst.mono { ((dm.bk >> 8) as u8 & 1, (dm.text >> 8) as u8 & 1) } else { (dm.bk as u8, dm.text as u8) };
        let src_bk = src.map(|s| if s.mono { (dm.bk >> 8) as u8 & 1 } else { dm.bk as u8 });
        let solid = brush.map(|b| (b.kind == BrushKind::Pattern && b.colour.iter().all(|&c| c == b.colour[0]) && b.mono.iter().all(|&m| m == b.mono[0])).then(|| if dst.mono { b.mono[0] & 1 } else { b.colour[0] })).flatten();
        let mut srow = vec![0u8; n];
        let mut drow = vec![0u8; n];
        let mut out = vec![0u8; n];
        for k in order {
            let y = dy + k;
            if let Some(s) = src {
                self.dev_read(s, sy + k, sx, &mut srow);
                if s.mono != dst.mono {
                    let sb = src_bk.unwrap_or(0);
                    for v in srow.iter_mut() {
                        *v = if s.mono {
                            if *v & 1 != 0 { bk } else { text }
                        } else if *v == sb {
                            0xff
                        } else {
                            0
                        };
                    }
                } else if s.mono {
                    for v in srow.iter_mut() {
                        *v = if *v & 1 != 0 { 0xff } else { 0 };
                    }
                }
            }
            if uses_d {
                self.dev_read(dst, y, dx, &mut drow);
                if dst.mono {
                    for v in drow.iter_mut() {
                        *v = if *v & 1 != 0 { 0xff } else { 0 };
                    }
                }
            }
            let (prow, keep) = match (uses_p, brush) {
                (true, Some(b)) if solid.is_none() => b.row(y as usize, dst.mono, dm, false),
                _ => ([0u8; 8], 0),
            };
            let pconst = match solid {
                Some(v) if dst.mono => if v & 1 != 0 { 0xff } else { 0 },
                Some(v) => v,
                None => 0,
            };
            let _ = keep;
            for i in 0..n {
                let p = if solid.is_some() || brush.is_none() {
                    pconst
                } else {
                    let v = prow[(dx as usize + i) & 7];
                    if dst.mono { if v & 1 != 0 { 0xff } else { 0 } } else { v }
                };
                let r = rop3(rop, p, srow[i], drow[i]);
                out[i] = if dst.mono { r >> 7 & 1 } else { r };
            }
            self.dev_write(dst, y, dx, &out);
        }
        if dst.screen {
            self.disp.touch(Rect::new(dx, dy, dx + w, dy + h));
        }
    }

    /// Fill a rectangle of `dst` with a brush under a ROP2 (scan lines,
    /// FASTBORDER), clipped to `clip`; transparent hatch backgrounds are
    /// left alone.
    pub(crate) fn fill_rop2(&mut self, dst: &Dev, r: Rect, brush: &Brush, dm: &DrawMode) {
        let r = r.intersect(&dst.bounds());
        if r.is_empty() {
            return;
        }
        let n = (r.x1 - r.x0) as usize;
        let mut row = vec![0u8; n];
        for y in r.y0..r.y1 {
            let (pat, keep) = brush.row(y as usize, dst.mono, dm, true);
            if dm.rop2 != 13 || keep != 0 {
                self.dev_read(dst, y, r.x0, &mut row);
            }
            for (i, v) in row.iter_mut().enumerate() {
                let x = (r.x0 as usize + i) & 7;
                if keep & (0x80 >> x) != 0 {
                    continue;
                }
                let p = pat[x];
                *v = if dst.mono { rop2(dm.rop2, if p & 1 != 0 { 0xff } else { 0 }, if *v & 1 != 0 { 0xff } else { 0 }) >> 7 & 1 } else { rop2(dm.rop2, p, *v) };
            }
            self.dev_write(dst, y, r.x0, &row);
        }
        if dst.screen {
            self.disp.touch(r);
        }
    }
}

/// A solid colour for pixel values of a destination.
pub(crate) fn pen_value(p: u32, mono: bool) -> u8 {
    if mono { (p >> 8) as u8 & 1 } else { p as u8 }
}

#[allow(dead_code)]
pub(crate) fn nearest_index(c: u32) -> u8 {
    nearest(c)
}
