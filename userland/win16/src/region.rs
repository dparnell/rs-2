//! GDI's regions: sets of non-overlapping rectangles in Windows
//! coordinates (left, top, right, bottom; right and bottom exclusive),
//! combined band by band. Ellipses and polygons become one rectangle per
//! scan line. A clip region is a PM region made from the rectangles when
//! it is selected (SelectClipRgn).

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::gdi::{self, Obj};
use crate::pm::*;

pub type Rects = Vec<[i32; 4]>;

const NULLREGION: u32 = 1;
const SIMPLEREGION: u32 = 2;
const COMPLEXREGION: u32 = 3;

fn kind(r: &Rects) -> u32 {
    match r.len() {
        0 => NULLREGION,
        1 => SIMPLEREGION,
        _ => COMPLEXREGION,
    }
}

fn rect(l: i32, t: i32, r: i32, b: i32) -> Rects {
    if r > l && b > t { alloc::vec![[l, t, r, b]] } else { Vec::new() }
}

/// The x intervals of `r` covering the band from y0 to y1 (sorted,
/// merged).
fn spans(r: &Rects, y0: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut v: Vec<(i32, i32)> = r.iter().filter(|q| q[1] <= y0 && q[3] >= y1).map(|q| (q[0], q[2])).collect();
    v.sort_unstable();
    let mut out: Vec<(i32, i32)> = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Combine two rectangle sets: 1 AND, 2 OR, 3 XOR, 4 DIFF.
pub fn combine(a: &Rects, b: &Rects, op: u16) -> Rects {
    let mut ys: Vec<i32> = a.iter().chain(b.iter()).flat_map(|r| [r[1], r[3]]).collect();
    ys.sort_unstable();
    ys.dedup();
    let mut out: Rects = Vec::new();
    // The previous band's spans and the rectangles made for it, to grow
    // them down when the next band has the same spans.
    let mut prev: Vec<(i32, i32)> = Vec::new();
    let mut prev_at = 0;
    let mut prev_end = i32::MIN;
    for w in ys.windows(2) {
        let (y0, y1) = (w[0], w[1]);
        let (sa, sb) = (spans(a, y0, y1), spans(b, y0, y1));
        let mut xs: Vec<i32> = sa.iter().chain(sb.iter()).flat_map(|s| [s.0, s.1]).collect();
        xs.sort_unstable();
        xs.dedup();
        let inside = |s: &[(i32, i32)], x: i32| s.iter().any(|&(l, r)| l <= x && x < r);
        let mut cur: Vec<(i32, i32)> = Vec::new();
        for x in xs.windows(2) {
            let (ia, ib) = (inside(&sa, x[0]), inside(&sb, x[0]));
            let on = match op {
                1 => ia && ib,
                2 => ia || ib,
                3 => ia != ib,
                _ => ia && !ib,
            };
            if on {
                match cur.last_mut() {
                    Some(l) if l.1 == x[0] => l.1 = x[1],
                    _ => cur.push((x[0], x[1])),
                }
            }
        }
        if cur == prev && prev_end == y0 && !cur.is_empty() {
            for r in &mut out[prev_at..] {
                r[3] = y1;
            }
        } else {
            prev_at = out.len();
            out.extend(cur.iter().map(|&(l, r)| [l, y0, r, y1]));
        }
        prev = cur;
        prev_end = y1;
    }
    out
}

/// An ellipse in a rectangle, one rectangle per scan line.
fn ellipse(l: i32, t: i32, r: i32, b: i32) -> Rects {
    let (w, h) = (r - l, b - t);
    if w <= 0 || h <= 0 {
        return Vec::new();
    }
    let (cx, cy) = ((l + r) as f32 / 2.0, (t + b) as f32 / 2.0);
    let (rx, ry) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut v = Vec::new();
    for y in t..b {
        let dy = (y as f32 + 0.5 - cy) / ry;
        let k = 1.0 - dy * dy;
        if k <= 0.0 {
            continue;
        }
        let dx = rx * sqrt(k);
        let (x0, x1) = ((cx - dx + 0.5) as i32, (cx + dx + 0.5) as i32);
        if x1 > x0 {
            v.push([x0, y, x1, y + 1]);
        }
    }
    combine(&v, &Vec::new(), 2)
}

fn sqrt(v: f32) -> f32 {
    let r: f32;
    unsafe { core::arch::asm!("sqrtss {0}, {1}", out(xmm_reg) r, in(xmm_reg) v, options(nostack, pure, nomem)) };
    r
}

/// Polygons (even-odd, mode 1, or winding, mode 2), one rectangle per
/// scan line.
fn polygons(polys: &[Vec<(i32, i32)>], mode: u16) -> Rects {
    let (mut top, mut bottom) = (i32::MAX, i32::MIN);
    for p in polys {
        for &(_, y) in p {
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    let mut v = Vec::new();
    for y in top..bottom {
        let yc = y as f32 + 0.5;
        let mut xs: Vec<(f32, i32)> = Vec::new();
        for p in polys {
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                if a.1 == b.1 {
                    continue;
                }
                let (lo, hi) = if a.1 < b.1 { (a, b) } else { (b, a) };
                if yc < lo.1 as f32 || yc >= hi.1 as f32 {
                    continue;
                }
                let x = lo.0 as f32 + (yc - lo.1 as f32) * (hi.0 - lo.0) as f32 / (hi.1 - lo.1) as f32;
                xs.push((x, if a.1 < b.1 { 1 } else { -1 }));
            }
        }
        xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
        let mut wind = 0;
        for k in 0..xs.len() {
            let was = if mode == 2 { wind != 0 } else { wind % 2 != 0 };
            wind += if mode == 2 { xs[k].1 } else { 1 };
            let _ = was;
            let now = if mode == 2 { wind != 0 } else { wind % 2 != 0 };
            if now && k + 1 < xs.len() {
                let (x0, x1) = ((xs[k].0 + 0.5) as i32, (xs[k + 1].0 + 0.5) as i32);
                if x1 > x0 {
                    v.push([x0, y, x1, y + 1]);
                }
            }
        }
    }
    combine(&v, &Vec::new(), 2)
}

fn region(h: u16) -> Option<Rects> {
    match gdi::obj(h) {
        Some(Obj::Region(r)) => Some(r),
        _ => None,
    }
}

pub fn set(h: u16, r: Rects) -> u32 {
    match gdi::obj_mut(h) {
        Some(Obj::Region(x)) => {
            let k = kind(&r);
            *x = r;
            k
        }
        _ => 0,
    }
}

pub fn new(r: Rects) -> u16 {
    gdi::new_obj(Obj::Region(r))
}

pub fn rects(h: u16) -> Option<Rects> {
    region(h)
}

fn bounds(r: &Rects) -> [i32; 4] {
    if r.is_empty() {
        return [0; 4];
    }
    r.iter().fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |b, q| [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[2]), b[3].max(q[3])])
}

/// SelectClipRgn(hdc, hrgn): the DC's clip region becomes a copy of the
/// region (0: none). The region's type.
pub fn select_clip(hdc: u16, h: u16) -> u32 {
    let Some(d) = gdi::dc(hdc) else { return 0 };
    let mut old = 0;
    if h == 0 {
        unsafe { GpiSetClipRegion(d.hps, 0, &mut old) };
        if old != 0 {
            unsafe { GpiDestroyRegion(d.hps, old) };
        }
        d.clipped = false;
        return SIMPLEREGION;
    }
    let Some(r) = region(h) else { return 0 };
    let pm: Vec<RectL> = r.iter().map(|q| gdi::pm_rect_dev(d, *q)).collect();
    let rgn = unsafe { GpiCreateRegion(d.hps, pm.len() as i32, pm.as_ptr()) };
    if rgn == 0 {
        return 0;
    }
    unsafe { GpiSetClipRegion(d.hps, rgn, &mut old) };
    if old != 0 {
        unsafe { GpiDestroyRegion(d.hps, old) };
    }
    d.clipped = true;
    kind(&r)
}

fn read_points(p: *const u8, n: usize) -> Vec<(i32, i32)> {
    (0..n)
        .map(|i| unsafe { ((p.add(4 * i) as *const i16).read_unaligned() as i32, (p.add(4 * i + 2) as *const i16).read_unaligned() as i32) })
        .collect()
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // FillRgn(hdc, hrgn, hbr), PaintRgn(hdc, hrgn): the DC's brush.
        3040 | 3043 => {
            let (h, r) = (a.w(), a.w());
            let br = if ord == 3040 { a.w() } else { gdi::brush_of(h) };
            let Some(r) = region(r) else { return Some(0) };
            for q in r {
                gdi::fill_rect(h, q, br);
            }
            1
        }
        // FrameRgn(hdc, hrgn, hbr, w, h): the region less itself shrunk.
        3041 => {
            let (h, r, br, w, hh) = (a.w(), a.w(), a.w(), a.i(), a.i());
            let Some(r) = region(r) else { return Some(0) };
            let inner: Rects = r.iter().map(|q| [q[0] + w, q[1] + hh, q[2] - w, q[3] - hh]).filter(|q| q[2] > q[0] && q[3] > q[1]).collect();
            for q in combine(&r, &combine(&inner, &Vec::new(), 2), 4) {
                gdi::fill_rect(h, q, br);
            }
            1
        }
        // InvertRgn(hdc, hrgn)
        3042 => {
            let (h, r) = (a.w(), a.w());
            let Some(r) = region(r) else { return Some(0) };
            for q in r {
                gdi::invert(h, q);
            }
            1
        }
        // SelectClipRgn(hdc, hrgn)
        3044 => {
            let (h, r) = (a.w(), a.w());
            select_clip(h, r)
        }
        // CombineRgn(dst, src1, src2, mode)
        3047 => {
            let (d, s1, s2, m) = (a.w(), a.w(), a.w(), a.w());
            let Some(r1) = region(s1) else { return Some(0) };
            let r = if m == 5 {
                r1
            } else {
                let Some(r2) = region(s2) else { return Some(0) };
                combine(&r1, &r2, m)
            };
            set(d, r)
        }
        // CreateEllipticRgn(l, t, r, b), CreateEllipticRgnIndirect(rect)
        3054 => {
            let (l, t, r, b) = (a.i(), a.i(), a.i(), a.i());
            new(ellipse(l, t, r, b)) as u32
        }
        3055 => {
            let r = crate::read_rect(a.l()).unwrap_or([0; 4]);
            new(ellipse(r[0], r[1], r[2], r[3])) as u32
        }
        // CreatePolygonRgn(points, n, mode)
        3063 => {
            let (p, n, m) = (a.p(), a.i(), a.w());
            if p.is_null() || n < 3 {
                return Some(new(Vec::new()) as u32);
            }
            new(polygons(&[read_points(p, n as usize)], m)) as u32
        }
        // CreatePolyPolygonRgn(points, counts, n, mode)
        3451 => {
            let (p, c, n, m) = (a.p(), a.p(), a.i(), a.w());
            if p.is_null() || c.is_null() {
                return Some(0);
            }
            let mut polys = Vec::new();
            let mut at = 0;
            for i in 0..n.max(0) as usize {
                let k = unsafe { (c.add(2 * i) as *const i16).read_unaligned() } as usize;
                polys.push(read_points(unsafe { p.add(4 * at) }, k));
                at += k;
            }
            new(polygons(&polys, m)) as u32
        }
        // CreateRectRgn(l, t, r, b), CreateRectRgnIndirect(rect)
        3064 => {
            let (l, t, r, b) = (a.i(), a.i(), a.i(), a.i());
            new(rect(l, t, r, b)) as u32
        }
        3065 => {
            let r = crate::read_rect(a.l()).unwrap_or([0; 4]);
            new(rect(r[0], r[1], r[2], r[3])) as u32
        }
        // CreateRoundRectRgn(l, t, r, b, w, h): the rectangle with
        // elliptic corners.
        3444 => {
            let (l, t, r, b, w, h) = (a.i(), a.i(), a.i(), a.i(), a.i(), a.i());
            let (w, h) = (w.min(r - l), h.min(b - t));
            let mut v = combine(&rect(l + w / 2, t, r - w / 2, b), &rect(l, t + h / 2, r, b - h / 2), 2);
            for (x, y) in [(l, t), (r - w, t), (l, b - h), (r - w, b - h)] {
                v = combine(&v, &ellipse(x, y, x + w, y + h), 2);
            }
            new(v) as u32
        }
        // EqualRgn(a, b)
        3072 => {
            let (x, y) = (a.w(), a.w());
            match (region(x), region(y)) {
                (Some(p), Some(q)) => combine(&p, &q, 3).is_empty() as u32,
                _ => 0,
            }
        }
        // OffsetRgn(hrgn, dx, dy)
        3101 => {
            let (h, dx, dy) = (a.w(), a.i(), a.i());
            let Some(r) = region(h) else { return Some(0) };
            set(h, r.iter().map(|q| [q[0] + dx, q[1] + dy, q[2] + dx, q[3] + dy]).collect())
        }
        // GetRgnBox(hrgn, rect)
        3134 => {
            let (h, p) = (a.w(), a.l());
            let Some(r) = region(h) else { return Some(0) };
            crate::write_rect(p, bounds(&r));
            kind(&r)
        }
        // PtInRegion(hrgn, x, y)
        3161 => {
            let (h, x, y) = (a.w(), a.i(), a.i());
            region(h).is_some_and(|r| r.iter().any(|q| x >= q[0] && x < q[2] && y >= q[1] && y < q[3])) as u32
        }
        // SetRectRgn(hrgn, l, t, r, b)
        3172 => {
            let (h, l, t, r, b) = (a.w(), a.i(), a.i(), a.i(), a.i());
            set(h, rect(l, t, r, b));
            0
        }
        // RectInRegion(hrgn, rect)
        3181 => {
            let (h, p) = (a.w(), a.l());
            let q = crate::read_rect(p).unwrap_or([0; 4]);
            region(h).is_some_and(|r| !combine(&r, &rect(q[0], q[1], q[2], q[3]), 1).is_empty()) as u32
        }
        _ => return None,
    })
}
