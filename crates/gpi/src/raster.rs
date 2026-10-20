//! Rasterisation: polygon scan conversion, lines, wide strokes, and the
//! curve approximations (ellipses, arcs, Bézier splines, fillets) that
//! feed them. Coordinates here are device pixels as f64.

use alloc::vec::Vec;

use crate::math::{abs, atan2, ceil, round, sin_cos, sqrt, PI};
use crate::region::{Region, Spans};

pub type Pf = (f64, f64);

/// Scan-convert closed figures: pixels whose centres are inside, by the
/// alternate (even-odd) or winding (non-zero) rule.
pub fn scan(figs: &[Vec<Pf>], winding: bool, out: &mut Spans) {
    let mut edges: Vec<(Pf, Pf)> = Vec::new();
    for f in figs {
        if f.len() < 2 {
            continue;
        }
        for i in 0..f.len() {
            let (a, b) = (f[i], f[(i + 1) % f.len()]);
            if a.1 != b.1 {
                edges.push((a, b));
            }
        }
    }
    if edges.is_empty() {
        return;
    }
    let ymin = edges.iter().map(|e| e.0.1.min(e.1.1)).fold(f64::MAX, f64::min);
    let ymax = edges.iter().map(|e| e.0.1.max(e.1.1)).fold(f64::MIN, f64::max);
    let mut xs: Vec<(f64, i32)> = Vec::new();
    let y0 = ceil(ymin - 0.5) as i32;
    let y1 = ceil(ymax - 0.5) as i32;
    for y in y0..y1 {
        let yc = y as f64 + 0.5;
        xs.clear();
        for &(a, b) in &edges {
            let (lo, hi) = if a.1 < b.1 { (a, b) } else { (b, a) };
            if yc >= lo.1 && yc < hi.1 {
                let x = lo.0 + (yc - lo.1) * (hi.0 - lo.0) / (hi.1 - lo.1);
                xs.push((x, if b.1 > a.1 { 1 } else { -1 }));
            }
        }
        xs.sort_unstable_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(core::cmp::Ordering::Equal));
        let px = |x: f64| ceil(x - 0.5) as i32;
        if winding {
            let mut w = 0;
            let mut start = 0.0;
            for &(x, d) in &xs {
                let was = w;
                w += d;
                if was == 0 && w != 0 {
                    start = x;
                } else if was != 0 && w == 0 {
                    out.add(y, px(start), px(x));
                }
            }
        } else {
            for pair in xs.chunks(2) {
                if let [a, b] = pair {
                    out.add(y, px(a.0), px(b.0));
                }
            }
        }
    }
}

/// Bresenham line between integer points, both ends included.
pub fn line(a: (i32, i32), b: (i32, i32), mut f: impl FnMut(i32, i32)) {
    line_clip(a, b, ALL, |_, x, y| f(x, y));
}

/// A window covering every i32 point.
pub const ALL: [i64; 4] = [i32::MIN as i64, i32::MIN as i64, i32::MAX as i64, i32::MAX as i64];

/// The pixels of `line(a, b)` inside the inclusive window `win`
/// (x0, y0, x1, y1), each with its step index along the whole line;
/// returns the number of pixels of the whole line. Only the steps in the
/// window are visited, so a line between far-off points (an
/// application's garbage coordinates) costs no more than the window.
///
/// Step `i` moves the major axis by `i` and the minor axis by
/// round(i * minor / major) with halves away from the start, which is
/// exactly where the error-term Bresenham lands.
pub fn line_clip(a: (i32, i32), b: (i32, i32), win: [i64; 4], mut f: impl FnMut(u64, i32, i32)) -> u64 {
    let (ax, ay, bx, by) = (a.0 as i64, a.1 as i64, b.0 as i64, b.1 as i64);
    let (adx, ady) = ((bx - ax).abs(), (by - ay).abs());
    let (sx, sy) = (if ax < bx { 1 } else { -1 }, if ay < by { 1 } else { -1 });
    let xmaj = adx >= ady;
    let (amaj, amin) = if xmaj { (adx, ady) } else { (ady, adx) };
    // Major and minor axes: start, direction, window.
    let (m0, ms, mlo, mhi, n0, ns, nlo, nhi) = if xmaj {
        (ax, sx, win[0], win[2], ay, sy, win[1], win[3])
    } else {
        (ay, sy, win[1], win[3], ax, sx, win[0], win[2])
    };
    let steps = amaj as u64 + 1;
    let (ilo, ihi) = if ms > 0 { (mlo - m0, mhi - m0) } else { (m0 - mhi, m0 - mlo) };
    let (ilo, ihi) = (ilo.max(0), ihi.min(amaj));
    if ilo > ihi {
        return steps;
    }
    let den = 2 * amaj.max(1);
    let num = 2 * ilo as i128 * amin as i128 + amaj as i128;
    let (mut q, mut r) = ((num / den as i128) as i64, (num % den as i128) as i64);
    for i in ilo..=ihi {
        let (m, n) = (m0 + ms * i, n0 + ns * q);
        if n >= nlo && n <= nhi {
            let (x, y) = if xmaj { (m, n) } else { (n, m) };
            f(i as u64, x as i32, y as i32);
        }
        r += 2 * amin;
        if r >= den {
            r -= den;
            q += 1;
        }
    }
    steps
}

pub fn ipt(p: Pf) -> (i32, i32) {
    (round(p.0), round(p.1))
}

/// Outline pixels of figures (closed or open polylines).
pub fn outline(figs: &[Vec<Pf>], closed: bool, out: &mut Spans) {
    for f in figs {
        if f.len() == 1 {
            let (x, y) = ipt(f[0]);
            out.add(y, x, x + 1);
        }
        let n = if closed && f.len() > 2 { f.len() } else { f.len().saturating_sub(1) };
        for i in 0..n {
            line(ipt(f[i]), ipt(f[(i + 1) % f.len()]), |x, y| out.add(y, x, x + 1));
        }
    }
}

/// Filled area including its boundary (PM's inclusive fills).
pub fn fill_region(figs: &[Vec<Pf>], winding: bool, incl: bool) -> Region {
    let mut s = Spans::new();
    scan(figs, winding, &mut s);
    if incl {
        outline(figs, true, &mut s);
    }
    s.into_region()
}

/// Number of segments for a curve of radius `r` pixels covering `sweep`
/// radians.
fn segments(r: f64, sweep: f64) -> usize {
    let n = (abs(sweep) * sqrt(r.max(1.0)) * 1.6) as usize;
    n.clamp(4, 720)
}

/// Points of an arc around `c` of the ellipse given by the conjugate
/// radius vectors `u` (angle 0) and `v` (angle 90°), from `start` over
/// `sweep` radians (positive = from u towards v).
pub fn arc_points(c: Pf, u: Pf, v: Pf, start: f64, sweep: f64, out: &mut Vec<Pf>) {
    let r = sqrt(u.0 * u.0 + u.1 * u.1).max(sqrt(v.0 * v.0 + v.1 * v.1));
    let n = segments(r, sweep);
    for i in 0..=n {
        let t = start + sweep * i as f64 / n as f64;
        let (s, co) = sin_cos(t);
        out.push((c.0 + u.0 * co + v.0 * s, c.1 + u.1 * co + v.1 * s));
    }
}

/// Cubic Bézier from p0 to p3 (p0 itself not pushed).
pub fn bezier(p0: Pf, p1: Pf, p2: Pf, p3: Pf, out: &mut Vec<Pf>) {
    let len = dist(p0, p1) + dist(p1, p2) + dist(p2, p3);
    let n = ((len / 3.0) as usize).clamp(2, 256);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let m = 1.0 - t;
        let (a, b, c, d) = (m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t);
        out.push((a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1));
    }
}

/// Quadratic Bézier from p0 to p2 with control p1 (p0 not pushed).
pub fn quad(p0: Pf, p1: Pf, p2: Pf, out: &mut Vec<Pf>) {
    let len = dist(p0, p1) + dist(p1, p2);
    let n = ((len / 3.0) as usize).clamp(2, 256);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let m = 1.0 - t;
        let (a, b, c) = (m * m, 2.0 * m * t, t * t);
        out.push((a * p0.0 + b * p1.0 + c * p2.0, a * p0.1 + b * p1.1 + c * p2.1));
    }
}

/// GpiPolyFillet: curves tangent to the lines joining the points, from
/// the first point to the last (first point not pushed).
pub fn fillet(pts: &[Pf], out: &mut Vec<Pf>) {
    match pts.len() {
        0 | 1 => {}
        2 => out.push(pts[1]),
        n => {
            let mid = |a: Pf, b: Pf| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let mut start = pts[0];
            for i in 1..n - 1 {
                let end = if i == n - 2 { pts[n - 1] } else { mid(pts[i], pts[i + 1]) };
                quad(start, pts[i], end, out);
                start = end;
            }
        }
    }
}

pub fn dist(a: Pf, b: Pf) -> f64 {
    sqrt((a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1))
}

/// Circle through three points: centre and radius (None if collinear).
pub fn circle3(a: Pf, b: Pf, c: Pf) -> Option<(Pf, f64)> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if abs(d) < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (a.0 * a.0 + a.1 * a.1, b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    let ux = (a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d;
    let uy = (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d;
    Some(((ux, uy), dist((ux, uy), a)))
}

/// Angles for the arc from a through b to c around centre `o`: start and
/// signed sweep.
pub fn arc3_angles(o: Pf, a: Pf, b: Pf, c: Pf) -> (f64, f64) {
    let ang = |p: Pf| atan2(p.1 - o.1, p.0 - o.0);
    let (ta, tb, tc) = (ang(a), ang(b), ang(c));
    let norm = |x: f64| {
        let mut x = x;
        while x < 0.0 {
            x += 2.0 * PI;
        }
        while x >= 2.0 * PI {
            x -= 2.0 * PI;
        }
        x
    };
    // Counter-clockwise sweep from a to c; if b is not on it, go clockwise.
    let ccw = norm(tc - ta);
    if norm(tb - ta) <= ccw { (ta, ccw) } else { (ta, ccw - 2.0 * PI) }
}

/// A wide stroke of a polyline as fillable polygons (segments as
/// rectangles, round joins and caps as polygons); fill with winding.
pub fn wide_stroke(pts: &[Pf], closed: bool, width: f64, out: &mut Vec<Vec<Pf>>) {
    let h = width / 2.0;
    let n = pts.len();
    if n == 0 {
        return;
    }
    let segs = if closed && n > 2 { n } else { n - 1 };
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let len = dist(a, b);
        if len == 0.0 {
            continue;
        }
        let (nx, ny) = (-(b.1 - a.1) / len * h, (b.0 - a.0) / len * h);
        // Consistent orientation so winding never cancels.
        out.push(alloc::vec![(a.0 + nx, a.1 + ny), (a.0 - nx, a.1 - ny), (b.0 - nx, b.1 - ny), (b.0 + nx, b.1 + ny)]);
    }
    for &p in pts {
        let mut disc = Vec::new();
        arc_points(p, (h, 0.0), (0.0, h), 0.0, 2.0 * PI, &mut disc);
        disc.pop();
        out.push(disc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rect::{Point, Rect};
    use alloc::vec;

    /// The error-term Bresenham `line` used to be.
    fn bresenham(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        let (mut x0, mut y0) = a;
        let (dx, dy) = ((b.0 - x0).abs(), -(b.1 - y0).abs());
        let (sx, sy) = (if x0 < b.0 { 1 } else { -1 }, if y0 < b.1 { 1 } else { -1 });
        let mut err = dx + dy;
        loop {
            out.push((x0, y0));
            if x0 == b.0 && y0 == b.1 {
                return out;
            }
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

    #[test]
    fn line_matches_bresenham() {
        for ax in -3..=3 {
            for ay in -3..=3 {
                for bx in -9..=9 {
                    for by in -9..=9 {
                        let mut got = Vec::new();
                        let n = line_clip((ax, ay), (bx, by), ALL, |i, x, y| {
                            assert_eq!(i as usize, got.len());
                            got.push((x, y));
                        });
                        let want = bresenham((ax, ay), (bx, by));
                        assert_eq!(got, want, "{:?} -> {:?}", (ax, ay), (bx, by));
                        assert_eq!(n as usize, want.len());
                    }
                }
            }
        }
    }

    #[test]
    fn line_clip_keeps_step_indices() {
        let win = [-2, -1, 4, 3];
        let mut seen = 0;
        for (a, b) in [((-20, -7), (30, 11)), ((15, 40), (-3, -35)), ((-50, 2), (50, 2)), ((1, -60), (1, 60))] {
            let want: Vec<(u64, i32, i32)> = bresenham(a, b)
                .into_iter()
                .enumerate()
                .filter(|&(_, (x, y))| (-2..=4).contains(&x) && (-1..=3).contains(&y))
                .map(|(i, (x, y))| (i as u64, x, y))
                .collect();
            let mut got = Vec::new();
            line_clip(a, b, win, |i, x, y| got.push((i, x, y)));
            seen += want.len();
            assert_eq!(got, want);
        }
        assert!(seen > 10);
    }

    #[test]
    fn line_clip_huge() {
        let mut got = Vec::new();
        let n = line_clip((-2147482763, 916), (1879049159, 916), [0, 900, 9, 930], |i, x, y| got.push((i, x, y)));
        assert_eq!(n, 1879049159u64 + 2147482763 + 1);
        let want: Vec<_> = (0..10).map(|x| (2147482763u64 + x as u64, x, 916)).collect();
        assert_eq!(got, want);
        // Diagonal across the whole i32 range.
        got.clear();
        line_clip((i32::MIN, i32::MIN), (i32::MAX, i32::MAX), [0, 0, 3, 3], |i, x, y| got.push((i, x, y)));
        let want: Vec<_> = (0..4).map(|k| (2147483648u64 + k as u64, k, k)).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn square_fill() {
        let sq = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        let g = fill_region(&[sq.clone()], false, false);
        assert_eq!(g, Region::from_rect(Rect::new(0, 0, 4, 4)));
        // Inclusive adds the right and top boundary pixels.
        let g = fill_region(&[sq], false, true);
        assert_eq!(g, Region::from_rect(Rect::new(0, 0, 5, 5)));
    }

    #[test]
    fn alternate_vs_winding() {
        // Two nested squares, same orientation: alternate leaves a hole.
        let outer = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let inner = vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)];
        let alt = fill_region(&[outer.clone(), inner.clone()], false, false);
        let win = fill_region(&[outer, inner], true, false);
        assert!(!alt.contains(Point::new(5, 5)));
        assert!(win.contains(Point::new(5, 5)));
        assert_eq!(win.area(), 100);
        assert_eq!(alt.area(), 84);
    }

    #[test]
    fn circle_area() {
        let mut c = Vec::new();
        arc_points((50.0, 50.0), (20.0, 0.0), (0.0, 20.0), 0.0, 2.0 * PI, &mut c);
        let g = fill_region(&[c], false, false);
        let a = g.area() as f64;
        assert!((a - PI * 400.0).abs() < 30.0, "{a}");
    }

    #[test]
    fn three_point_circle() {
        let (o, r) = circle3((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)).unwrap();
        assert!(o.0.abs() < 1e-9 && o.1.abs() < 1e-9 && (r - 1.0).abs() < 1e-9);
        let (s, sw) = arc3_angles(o, (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0));
        assert!(s.abs() < 1e-9 && (sw - PI).abs() < 1e-9);
        let (_, sw) = arc3_angles(o, (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0));
        assert!((sw + PI).abs() < 1e-9);
        assert!(circle3((0.0, 0.0), (1.0, 1.0), (2.0, 2.0)).is_none());
    }
}
