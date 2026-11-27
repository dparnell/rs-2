//! GDI's regions: sets of non-overlapping rectangles in Windows
//! coordinates (left, top, right, bottom; right and bottom exclusive),
//! combined band by band. Ellipses and polygons become one rectangle per
//! scan line. A clip region is a PM region made from the rectangles when
//! it is selected (SelectClipRgn).

use alloc::vec::Vec;

use crate::gdi::{self, Obj};
use crate::pm::*;

pub type Rects = Vec<[i32; 4]>;

const NULLREGION: u32 = 1;
const SIMPLEREGION: u32 = 2;
const COMPLEXREGION: u32 = 3;

pub fn kind(r: &Rects) -> u32 {
    match r.len() {
        0 => NULLREGION,
        1 => SIMPLEREGION,
        _ => COMPLEXREGION,
    }
}

pub fn rect(l: i32, t: i32, r: i32, b: i32) -> Rects {
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
pub fn ellipse(l: i32, t: i32, r: i32, b: i32) -> Rects {
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
pub fn polygons(polys: &[Vec<(i32, i32)>], mode: u16) -> Rects {
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

pub fn region(h: u16) -> Option<Rects> {
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

pub fn bounds(r: &Rects) -> [i32; 4] {
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

/// POINTs (32-bit x and y).
pub fn read_points(p: *const u8, n: usize) -> Vec<(i32, i32)> {
    (0..n)
        .map(|i| unsafe { ((p.add(8 * i) as *const i32).read_unaligned(), (p.add(8 * i + 4) as *const i32).read_unaligned()) })
        .collect()
}
