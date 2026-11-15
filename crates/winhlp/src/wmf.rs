//! Windows metafiles (the pictures of type 8) drawn into bitmaps: the
//! records help files use (lines, polygons, rectangles, ellipses, arcs,
//! text, device independent bitmaps; pens, brushes and fonts as GDI
//! objects), on a white page the size the picture gives (HIMETRIC for
//! the anisotropic and isotropic mapping modes, at 96 dots per inch).
//!
//! A metafile is an 18-byte header (u16 type, u16 header words, u16
//! version, u32 size in words, u16 objects, u32 largest record, u16 0)
//! and records `u32 size in words, u16 function, u16 parameters[]`
//! (coordinates y before x, as GDI pushes them), ended by function 0.
//! Text is drawn with the 8×8 and 8×14 cells of unscii (public domain)
//! scaled to the font's height.

use crate::hfs::{u16_at, u32_at};
use alloc::vec;
use alloc::vec::Vec;

static UNSCII8: &[u8] = include_bytes!("../../../kernel/assets/unscii-8.bin");
static UNSCII14: &[u8] = include_bytes!("../../../kernel/assets/unscii-14.bin");

/// Largest bitmap made from a metafile, each way.
const MAX: u32 = 1600;

#[derive(Clone, Copy)]
struct Pen {
    null: bool,
    width: i32,
    colour: u32,
}

#[derive(Clone, Copy)]
struct Brush {
    null: bool,
    colour: u32,
}

#[derive(Clone, Copy)]
struct FontSpec {
    height: i32,
    bold: bool,
    italic: bool,
    underline: bool,
}

#[derive(Clone, Copy)]
enum Obj {
    Pen(Pen),
    Brush(Brush),
    Font(FontSpec),
    Other,
}

#[derive(Clone, Copy)]
struct Dc {
    org: (i32, i32),
    ext: (i32, i32),
    pen: Pen,
    brush: Brush,
    font: FontSpec,
    text: u32,
    bk: u32,
    opaque: bool,
    align: u16,
    winding: bool,
    pos: (f32, f32),
}

struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u32>,
}

impl Canvas {
    fn set(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    fn span(&mut self, y: i32, x0: i32, x1: i32, c: u32) {
        if y < 0 || y >= self.h {
            return;
        }
        let (a, b) = (x0.max(0), x1.min(self.w));
        for x in a..b {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    /// Fill polygons (even-odd or winding), sampled at pixel centres.
    fn fill(&mut self, polys: &[Vec<(f32, f32)>], winding: bool, c: u32) {
        let (mut ymin, mut ymax) = (f32::MAX, f32::MIN);
        for p in polys {
            for &(_, y) in p {
                ymin = ymin.min(y);
                ymax = ymax.max(y);
            }
        }
        if ymin > ymax {
            return;
        }
        let (y0, y1) = ((ymin.max(0.0)) as i32, (ymax.min(self.h as f32) + 1.0) as i32);
        let mut xs: Vec<(f32, i32)> = Vec::new();
        for y in y0..y1 {
            let sy = y as f32 + 0.5;
            xs.clear();
            for p in polys {
                for i in 0..p.len() {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    if (a.1 <= sy && b.1 > sy) || (b.1 <= sy && a.1 > sy) {
                        let x = a.0 + (sy - a.1) * (b.0 - a.0) / (b.1 - a.1);
                        xs.push((x, if b.1 > a.1 { 1 } else { -1 }));
                    }
                }
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
            let mut n = 0;
            for i in 0..xs.len() {
                let inside_before = if winding { n != 0 } else { n % 2 != 0 };
                n += if winding { xs[i].1 } else { 1 };
                let inside = if winding { n != 0 } else { n % 2 != 0 };
                if !inside_before && inside {
                    // Span start: to the next crossing that leaves.
                    let mut m = n;
                    let mut j = i + 1;
                    while j < xs.len() {
                        m += if winding { xs[j].1 } else { 1 };
                        if if winding { m == 0 } else { m % 2 == 0 } {
                            break;
                        }
                        j += 1;
                    }
                    if j < xs.len() {
                        let (a, b) = ((xs[i].0 + 0.5) as i32, (xs[j].0 + 0.5) as i32);
                        self.span(y, a, b, c);
                    }
                }
            }
        }
    }

    /// A line of `w` pixels.
    fn line(&mut self, a: (f32, f32), b: (f32, f32), w: i32, c: u32) {
        let (mut x0, mut y0) = (a.0.round_() as i32, a.1.round_() as i32);
        let (x1, y1) = (b.0.round_() as i32, b.1.round_() as i32);
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let mut err = dx + dy;
        let r = (w.max(1) - 1) / 2;
        let mut guard = 0;
        loop {
            if r == 0 {
                self.set(x0, y0, c);
            } else {
                for yy in -r..=r {
                    self.span(y0 + yy, x0 - r, x0 + r + 1, c);
                }
            }
            if (x0 == x1 && y0 == y1) || guard > 20000 {
                break;
            }
            guard += 1;
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn stroke(&mut self, pts: &[(f32, f32)], closed: bool, pen: &Pen) {
        if pen.null || pts.is_empty() {
            return;
        }
        for i in 1..pts.len() {
            self.line(pts[i - 1], pts[i], pen.width, pen.colour);
        }
        if closed && pts.len() > 2 {
            self.line(pts[pts.len() - 1], pts[0], pen.width, pen.colour);
        }
    }
}

trait Round {
    fn round_(self) -> f32;
}

impl Round for f32 {
    fn round_(self) -> f32 {
        if self >= 0.0 { (self + 0.5) as i32 as f32 } else { -((-self + 0.5) as i32 as f32) }
    }
}

/// sin and cos (Taylor series after reduction to ±π/4).
fn sin_cos(a: f32) -> (f32, f32) {
    let tau = core::f32::consts::TAU;
    let mut a = a % tau;
    if a < 0.0 {
        a += tau;
    }
    let q = ((a / core::f32::consts::FRAC_PI_2) + 0.5) as i32;
    let r = a - q as f32 * core::f32::consts::FRAC_PI_2;
    let r2 = r * r;
    let s = r * (1.0 - r2 / 6.0 * (1.0 - r2 / 20.0 * (1.0 - r2 / 42.0)));
    let c = 1.0 - r2 / 2.0 * (1.0 - r2 / 12.0 * (1.0 - r2 / 30.0));
    match q & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

fn atan2(y: f32, x: f32) -> f32 {
    use core::f32::consts::{FRAC_PI_2, PI};
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let (ax, ay) = (if x < 0.0 { -x } else { x }, if y < 0.0 { -y } else { y });
    let (t, swap) = if ay > ax { (ax / ay, true) } else { (ay / ax, false) };
    // atan on [0, 1].
    let t2 = t * t;
    let mut a = t * (0.999_866 + t2 * (-0.330_299 + t2 * (0.180_141 + t2 * (-0.085_133 + t2 * 0.020_835))));
    if swap {
        a = FRAC_PI_2 - a;
    }
    if x < 0.0 {
        a = PI - a;
    }
    if y < 0.0 { -a } else { a }
}

/// Points of the ellipse in the box (l, t, r, b) from angle `a0` for
/// `sweep` radians (counterclockwise on the screen).
fn ellipse_pts(l: f32, t: f32, r: f32, b: f32, a0: f32, sweep: f32, out: &mut Vec<(f32, f32)>) {
    let (cx, cy, rx, ry) = ((l + r) / 2.0, (t + b) / 2.0, (r - l).abs() / 2.0, (b - t).abs() / 2.0);
    let n = ((rx + ry) * sweep.abs() / 4.0).clamp(8.0, 256.0) as i32;
    for i in 0..=n {
        let a = a0 + sweep * i as f32 / n as f32;
        let (s, c) = sin_cos(a);
        out.push((cx + rx * c, cy - ry * s));
    }
}

/// A COLORREF (0x00BBGGRR) as 0xRRGGBB.
fn rgb(c: u32) -> u32 {
    (c & 0xff) << 16 | (c & 0xff00) | (c >> 16 & 0xff)
}

/// A glyph table: (code point, rows) records of a font file.
struct Glyphs {
    data: &'static [u8],
    rows: usize,
}

impl Glyphs {
    fn glyph(&self, cp: u32) -> Option<&'static [u8]> {
        let rec = 4 + self.rows;
        let n = self.data.len() / rec;
        let (mut lo, mut hi) = (0, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let c = u32_at(self.data, mid * rec)?;
            if c == cp {
                return self.data.get(mid * rec + 4..mid * rec + rec);
            }
            if c < cp {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        None
    }
}

/// Code page 1252's 0x80-0x9F as Unicode.
const C1: [u16; 32] = [
    0x20ac, 0x3f, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x3f, 0x17d, 0x3f, 0x3f, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a,
    0x153, 0x3f, 0x17e, 0x178,
];

fn unicode(c: u8) -> u32 {
    if (0x80..0xa0).contains(&c) { C1[(c - 0x80) as usize] as u32 } else { c as u32 }
}

/// Draw a metafile of `cx` by `cy` (HIMETRIC when `mode` is 7 or 8,
/// else pixels): the bitmap's width, height and pixels (0xRRGGBB, rows
/// top-down).
pub fn render(mode: u16, cx: u16, cy: u16, m: &[u8]) -> Option<(u32, u32, Vec<u32>)> {
    let (w, h) = if mode == 7 || mode == 8 { (cx as u32 * 96 / 2540, cy as u32 * 96 / 2540) } else { (cx as u32, cy as u32) };
    let (w, h) = (w.clamp(1, MAX), h.clamp(1, MAX));
    if u16_at(m, 2)? != 9 {
        return None;
    }
    let mut cv = Canvas { w: w as i32, h: h as i32, px: vec![0xffffff; (w * h) as usize] };
    let mut dc = Dc {
        org: (0, 0),
        ext: (w as i32, h as i32),
        pen: Pen { null: false, width: 1, colour: 0 },
        brush: Brush { null: false, colour: 0xffffff },
        font: FontSpec { height: 12, bold: false, italic: false, underline: false },
        text: 0,
        bk: 0xffffff,
        opaque: true,
        align: 0,
        winding: false,
        pos: (0.0, 0.0),
    };
    let mut saved: Vec<Dc> = Vec::new();
    let mut objs: Vec<Option<Obj>> = Vec::new();
    let small = Glyphs { data: UNSCII8, rows: 8 };
    let big = Glyphs { data: UNSCII14, rows: 14 };
    let mut p = 18;
    let mut count = 0;
    while p + 6 <= m.len() && count < 100_000 {
        count += 1;
        let size = u32_at(m, p)? as usize * 2;
        let func = u16_at(m, p + 4)?;
        if func == 0 || size < 6 {
            break;
        }
        let rec = m.get(p + 6..(p + size).min(m.len()))?;
        p += size;
        let a = |i: usize| -> i32 { u16_at(rec, 2 * i).unwrap_or(0) as i16 as i32 };
        let x = |dc: &Dc, v: i32| -> f32 { (v - dc.org.0) as f32 * w as f32 / if dc.ext.0 == 0 { 1.0 } else { dc.ext.0 as f32 } };
        let y = |dc: &Dc, v: i32| -> f32 { (v - dc.org.1) as f32 * h as f32 / if dc.ext.1 == 0 { 1.0 } else { dc.ext.1 as f32 } };
        let pt = |dc: &Dc, i: usize| -> (f32, f32) { (x(dc, a(i + 1)), y(dc, a(i))) };
        let new_obj = |objs: &mut Vec<Option<Obj>>, o: Obj| match objs.iter().position(|s| s.is_none()) {
            Some(i) => objs[i] = Some(o),
            None => objs.push(Some(o)),
        };
        let pen_w = |dc: &Dc| -> i32 { if dc.pen.width <= 1 { 1 } else { ((dc.pen.width as f32 * w as f32 / dc.ext.0.unsigned_abs().max(1) as f32) + 0.5) as i32 }.clamp(1, 16) };
        match func {
            0x020b => dc.org = (a(1), a(0)),
            0x020c => dc.ext = (a(1), a(0)),
            0x020f => dc.org = (dc.org.0 + a(1), dc.org.1 + a(0)),
            0x0209 => dc.text = rgb(u32_at(rec, 0).unwrap_or(0)),
            0x0201 => dc.bk = rgb(u32_at(rec, 0).unwrap_or(0)),
            0x0102 => dc.opaque = a(0) == 2,
            0x012e => dc.align = a(0) as u16,
            0x0106 => dc.winding = a(0) == 2,
            0x001e => saved.push(dc),
            0x0127 => {
                let n = a(0);
                let keep = if n < 0 { saved.len().saturating_sub((-n) as usize) } else { (n as usize).saturating_sub(1).min(saved.len()) };
                if keep < saved.len() {
                    dc = saved[keep];
                    saved.truncate(keep);
                }
            }
            0x02fa => {
                let style = a(0) & 0xf;
                new_obj(&mut objs, Obj::Pen(Pen { null: style == 5, width: a(1), colour: rgb(u32_at(rec, 6).unwrap_or(0)) }));
            }
            0x02fc => {
                let style = a(0);
                new_obj(&mut objs, Obj::Brush(Brush { null: style == 1, colour: rgb(u32_at(rec, 2).unwrap_or(0)) }));
            }
            0x02fb => {
                let hgt = (y(&dc, a(0)) - y(&dc, 0)).abs() as i32;
                let f = FontSpec { height: hgt.max(4), bold: a(4) >= 600, italic: rec.get(10).copied().unwrap_or(0) != 0, underline: rec.get(11).copied().unwrap_or(0) != 0 };
                new_obj(&mut objs, Obj::Font(f));
            }
            0x00f7 | 0x01f9 | 0x0142 | 0x06ff => new_obj(&mut objs, Obj::Other),
            0x012d => match objs.get(a(0) as usize).copied().flatten() {
                Some(Obj::Pen(pn)) => dc.pen = pn,
                Some(Obj::Brush(b)) => dc.brush = b,
                Some(Obj::Font(f)) => dc.font = f,
                _ => {}
            },
            0x01f0 => {
                if let Some(s) = objs.get_mut(a(0) as usize) {
                    *s = None;
                }
            }
            0x0214 => dc.pos = pt(&dc, 0),
            0x0213 => {
                let to = pt(&dc, 0);
                if !dc.pen.null {
                    cv.line(dc.pos, to, pen_w(&dc), dc.pen.colour);
                }
                dc.pos = to;
            }
            0x0325 | 0x0324 => {
                let n = a(0).max(0) as usize;
                let pts: Vec<(f32, f32)> = (0..n).map(|i| (x(&dc, a(1 + 2 * i)), y(&dc, a(2 + 2 * i)))).collect();
                let mut pen = dc.pen;
                pen.width = pen_w(&dc);
                if func == 0x0324 {
                    if !dc.brush.null {
                        cv.fill(&[pts.clone()], dc.winding, dc.brush.colour);
                    }
                    cv.stroke(&pts, true, &pen);
                } else {
                    cv.stroke(&pts, false, &pen);
                }
            }
            0x0538 => {
                let n = a(0).max(0) as usize;
                let counts: Vec<usize> = (0..n).map(|i| a(1 + i).max(0) as usize).collect();
                let mut k = 1 + n;
                let mut polys = Vec::new();
                for c in counts {
                    polys.push((0..c).map(|i| (x(&dc, a(k + 2 * i)), y(&dc, a(k + 1 + 2 * i)))).collect::<Vec<_>>());
                    k += 2 * c;
                }
                if !dc.brush.null {
                    cv.fill(&polys, dc.winding, dc.brush.colour);
                }
                let mut pen = dc.pen;
                pen.width = pen_w(&dc);
                for p in &polys {
                    cv.stroke(p, true, &pen);
                }
            }
            0x041b | 0x061c | 0x0418 => {
                let o = if func == 0x061c { 2 } else { 0 };
                let (b, r, t, l) = (y(&dc, a(o)), x(&dc, a(o + 1)), y(&dc, a(o + 2)), x(&dc, a(o + 3)));
                let mut pts = Vec::new();
                if func == 0x0418 {
                    ellipse_pts(l, t, r, b, 0.0, core::f32::consts::TAU, &mut pts);
                } else {
                    pts.extend_from_slice(&[(l, t), (r, t), (r, b), (l, b)]);
                }
                if !dc.brush.null {
                    cv.fill(&[pts.clone()], false, dc.brush.colour);
                }
                let mut pen = dc.pen;
                pen.width = pen_w(&dc);
                cv.stroke(&pts, true, &pen);
            }
            0x0817 | 0x081a | 0x0830 => {
                let (ye, xe, ys, xs) = (y(&dc, a(0)), x(&dc, a(1)), y(&dc, a(2)), x(&dc, a(3)));
                let (b, r, t, l) = (y(&dc, a(4)), x(&dc, a(5)), y(&dc, a(6)), x(&dc, a(7)));
                let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
                let a0 = atan2(cy - ys, xs - cx);
                let mut a1 = atan2(cy - ye, xe - cx);
                if a1 <= a0 {
                    a1 += core::f32::consts::TAU;
                }
                let mut pts = Vec::new();
                ellipse_pts(l, t, r, b, a0, a1 - a0, &mut pts);
                let mut pen = dc.pen;
                pen.width = pen_w(&dc);
                if func == 0x0817 {
                    cv.stroke(&pts, false, &pen);
                } else {
                    if func == 0x081a {
                        pts.push((cx, cy));
                    }
                    if !dc.brush.null {
                        cv.fill(&[pts.clone()], false, dc.brush.colour);
                    }
                    cv.stroke(&pts, true, &pen);
                }
            }
            0x061d => {
                // PatBlt: the brush over a rectangle.
                let (hh, ww, yy, xx) = (a(2), a(3), a(4), a(5));
                let (x0, y0, x1, y1) = (x(&dc, xx), y(&dc, yy), x(&dc, xx + ww), y(&dc, yy + hh));
                if !dc.brush.null {
                    cv.fill(&[vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]], false, dc.brush.colour);
                }
            }
            0x0521 | 0x0a32 => {
                let (at, s, dx, opaque_rect) = if func == 0x0521 {
                    let n = a(0).max(0) as usize;
                    let s = rec.get(2..2 + n).unwrap_or(&[]);
                    let o = 2 + n.div_ceil(2) * 2;
                    let yy = u16_at(rec, o).unwrap_or(0) as i16 as i32;
                    let xx = u16_at(rec, o + 2).unwrap_or(0) as i16 as i32;
                    ((x(&dc, xx), y(&dc, yy)), s, Vec::new(), None)
                } else {
                    let n = a(2).max(0) as usize;
                    let opts = a(3);
                    let mut o = 8;
                    let mut rect = None;
                    if opts & 0x06 != 0 {
                        rect = Some((x(&dc, a(4)), y(&dc, a(5)), x(&dc, a(6)), y(&dc, a(7))));
                        o += 8;
                    }
                    let s = rec.get(o..o + n).unwrap_or(&[]);
                    o += n.div_ceil(2) * 2;
                    let dx: Vec<f32> = if rec.len() >= o + 2 * n {
                        (0..n).map(|i| (u16_at(rec, o + 2 * i).unwrap_or(0) as i16 as f32) * w as f32 / dc.ext.0.unsigned_abs().max(1) as f32).collect()
                    } else {
                        Vec::new()
                    };
                    ((x(&dc, a(1)), y(&dc, a(0))), s, dx, if opts & 0x02 != 0 { rect } else { None })
                };
                if let Some((l, t, r, b)) = opaque_rect {
                    cv.fill(&[vec![(l, t), (r, t), (r, b), (l, b)]], false, dc.bk);
                }
                text(&mut cv, &dc, at, s, &dx, &small, &big);
            }
            0x0f43 | 0x0b41 | 0x0940 => {
                // StretchDIBits, DibStretchBlt, DibBitBlt.
                let (src, dst, dib) = match func {
                    0x0f43 => ((a(6), a(5), a(4), a(3)), (a(10), a(9), a(8), a(7)), rec.get(22..)),
                    0x0b41 => ((a(5), a(4), a(3), a(2)), (a(9), a(8), a(7), a(6)), rec.get(20..)),
                    _ => ((a(3), a(2), a(5), a(4)), (a(7), a(6), a(5), a(4)), rec.get(16..)),
                };
                // (x, y, w, h) of the source; destination likewise.
                let _ = src;
                if let Some(dib) = dib {
                    let (xd, yd, wd, hd) = dst;
                    let (x0, y0, x1, y1) = (x(&dc, xd), y(&dc, yd), x(&dc, xd + wd), y(&dc, yd + hd));
                    blit(&mut cv, dib, (x0, y0, x1, y1));
                }
            }
            _ => {}
        }
    }
    Some((w, h, cv.px))
}

/// Text at `at` (the text alignment's reference point), characters
/// `dx` apart when given.
fn text(cv: &mut Canvas, dc: &Dc, at: (f32, f32), s: &[u8], dx: &[f32], small: &Glyphs, big: &Glyphs) {
    let hgt = dc.font.height.clamp(4, 200);
    let (g, rows) = if hgt <= 10 { (small, 8) } else { (big, 14) };
    // About a proportional font's average width.
    let gw = (hgt as f32 * 0.46).max(4.0);
    let adv = |i: usize| -> f32 { dx.get(i).copied().unwrap_or(gw) };
    let total: f32 = (0..s.len()).map(adv).sum();
    let mut x0 = at.0;
    match dc.align & 6 {
        6 => x0 -= total / 2.0,
        2 => x0 -= total,
        _ => {}
    }
    // Reference: top (0), bottom (8) or baseline (24).
    let top = match dc.align & 24 {
        24 => at.1 - hgt as f32 * 0.8,
        8 => at.1 - hgt as f32,
        _ => at.1,
    };
    let mut cx = x0;
    for (i, &c) in s.iter().enumerate() {
        if let Some(glyph) = g.glyph(unicode(c)) {
            let gwi = gw as i32;
            for yy in 0..hgt {
                let row = glyph[(yy * rows / hgt) as usize];
                for xx in 0..gwi {
                    let col = (xx * 8 / gwi.max(1)) as u32;
                    if row & (0x80 >> col) != 0 {
                        let shear = if dc.font.italic { ((hgt - yy) / 4) as f32 } else { 0.0 };
                        let px = (cx + xx as f32 + shear) as i32;
                        let py = top as i32 + yy;
                        cv.set(px, py, dc.text);
                        if dc.font.bold {
                            cv.set(px + 1, py, dc.text);
                        }
                    }
                }
            }
            if dc.font.underline {
                cv.span(top as i32 + hgt - 1, cx as i32, (cx + adv(i)) as i32, dc.text);
            }
        }
        cx += adv(i);
    }
}

/// A device independent bitmap (BITMAPINFOHEADER or BITMAPCOREHEADER,
/// palette, bits) stretched over a rectangle.
fn blit(cv: &mut Canvas, dib: &[u8], (x0, y0, x1, y1): (f32, f32, f32, f32)) {
    let Some(hs) = u32_at(dib, 0) else { return };
    let (bw, bh, bpp, comp, used, pal_entry) = if hs == 12 {
        (u16_at(dib, 4).unwrap_or(0) as i32, u16_at(dib, 6).unwrap_or(0) as i32, u16_at(dib, 10).unwrap_or(0), 0, 0, 3)
    } else {
        (u32_at(dib, 4).unwrap_or(0) as i32, u32_at(dib, 8).unwrap_or(0) as i32, u16_at(dib, 14).unwrap_or(0), u32_at(dib, 16).unwrap_or(0), u32_at(dib, 32).unwrap_or(0), 4)
    };
    if bw <= 0 || bh == 0 || comp != 0 || !matches!(bpp, 1 | 4 | 8 | 24) {
        return;
    }
    let (bh, top_down) = if bh < 0 { (-bh, true) } else { (bh, false) };
    let colours = if bpp <= 8 { if used != 0 { used as usize } else { 1 << bpp } } else { 0 };
    let pal_at = hs as usize;
    let pal: Vec<u32> = (0..colours)
        .map(|i| {
            let o = pal_at + i * pal_entry;
            dib.get(o..o + 3).map_or(0, |q| (q[2] as u32) << 16 | (q[1] as u32) << 8 | q[0] as u32)
        })
        .collect();
    let bits = &dib[(pal_at + colours * pal_entry).min(dib.len())..];
    let stride = ((bw as usize * bpp as usize + 31) / 32) * 4;
    let (l, r) = (x0.min(x1), x0.max(x1));
    let (t, b) = (y0.min(y1), y0.max(y1));
    let (dw, dh) = ((r - l).max(1.0), (b - t).max(1.0));
    for dy in (t as i32).max(0)..(b as i32).min(cv.h) {
        let sy = (((dy as f32 + 0.5 - t) / dh) * bh as f32) as i32;
        let row = if top_down { sy } else { bh - 1 - sy };
        if row < 0 || row >= bh {
            continue;
        }
        let ro = row as usize * stride;
        for dx in (l as i32).max(0)..(r as i32).min(cv.w) {
            let sx = (((dx as f32 + 0.5 - l) / dw) * bw as f32) as usize;
            let c = match bpp {
                1 => bits.get(ro + sx / 8).map(|&v| pal.get((v >> (7 - sx % 8) & 1) as usize).copied().unwrap_or(0)),
                4 => bits.get(ro + sx / 2).map(|&v| pal.get((if sx % 2 == 0 { v >> 4 } else { v & 15 }) as usize).copied().unwrap_or(0)),
                8 => bits.get(ro + sx).map(|&v| pal.get(v as usize).copied().unwrap_or(0)),
                _ => bits.get(ro + 3 * sx..ro + 3 * sx + 3).map(|q| (q[2] as u32) << 16 | (q[1] as u32) << 8 | q[0] as u32),
            };
            if let Some(c) = c {
                cv.set(dx, dy, c);
            }
        }
    }
}

/// The metafile as a 24-bit (or, with at most 256 colours, 8-bit)
/// Windows bitmap file.
pub fn bmp(w: u32, h: u32, px: &[u32]) -> Vec<u8> {
    let mut pal: Vec<u32> = Vec::new();
    let mut idx: Vec<u8> = Vec::with_capacity(px.len());
    for &c in px {
        let i = match pal.iter().position(|&p| p == c) {
            Some(i) => i,
            None if pal.len() < 256 => {
                pal.push(c);
                pal.len() - 1
            }
            None => {
                pal.clear();
                break;
            }
        };
        idx.push(i as u8);
    }
    if !pal.is_empty() && idx.len() == px.len() {
        let stride = (w as usize + 3) & !3;
        let mut bits = vec![0u8; stride * h as usize];
        for y in 0..h as usize {
            let src = &idx[y * w as usize..(y + 1) * w as usize];
            let dst = (h as usize - 1 - y) * stride;
            bits[dst..dst + w as usize].copy_from_slice(src);
        }
        return crate::pict::bmp_file(w, h, 8, &pal, &bits);
    }
    let stride = (w as usize * 3 + 3) & !3;
    let mut bits = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let c = px[y * w as usize + x];
            let o = (h as usize - 1 - y) * stride + 3 * x;
            bits[o] = c as u8;
            bits[o + 1] = (c >> 8) as u8;
            bits[o + 2] = (c >> 16) as u8;
        }
    }
    crate::pict::bmp_file(w, h, 24, &[], &bits)
}
