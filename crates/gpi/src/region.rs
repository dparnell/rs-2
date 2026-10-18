//! Regions: sets of pixels kept as y-x bands (rows of disjoint x spans,
//! vertically adjacent identical bands merged), with boolean operations.
//! Used for GPI regions, clip regions and paths, and fill rasterisation.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::rect::{Point, Rect};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Band {
    y0: i32,
    y1: i32,
    /// Sorted, disjoint, non-touching [x0, x1) spans.
    spans: Vec<(i32, i32)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Region {
    bands: Vec<Band>,
}

/// Boolean combination modes (CRGN_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Or,
    And,
    Xor,
    Diff,
}

impl Op {
    fn eval(self, a: bool, b: bool) -> bool {
        match self {
            Op::Or => a | b,
            Op::And => a & b,
            Op::Xor => a ^ b,
            Op::Diff => a & !b,
        }
    }
}

/// Region complexity (RGN_NULL 1, RGN_RECT 2, RGN_COMPLEX 3).
pub const RGN_NULL: i32 = 1;
pub const RGN_RECT: i32 = 2;
pub const RGN_COMPLEX: i32 = 3;

fn combine_spans(a: &[(i32, i32)], b: &[(i32, i32)], op: Op, out: &mut Vec<(i32, i32)>) {
    let mut xs: Vec<i32> = Vec::with_capacity((a.len() + b.len()) * 2);
    for &(x0, x1) in a.iter().chain(b.iter()) {
        xs.push(x0);
        xs.push(x1);
    }
    xs.sort_unstable();
    xs.dedup();
    let (mut ia, mut ib) = (0, 0);
    let inside = |s: &[(i32, i32)], i: &mut usize, x: i32| {
        while *i < s.len() && s[*i].1 <= x {
            *i += 1;
        }
        *i < s.len() && s[*i].0 <= x
    };
    for w in xs.windows(2) {
        let (x0, x1) = (w[0], w[1]);
        if op.eval(inside(a, &mut ia, x0), inside(b, &mut ib, x0)) {
            match out.last_mut() {
                Some(last) if last.1 == x0 => last.1 = x1,
                _ => out.push((x0, x1)),
            }
        }
    }
}

/// Sort and merge overlapping/touching spans.
fn normalise(spans: &mut Vec<(i32, i32)>) {
    spans.retain(|s| s.0 < s.1);
    spans.sort_unstable();
    let mut out: Vec<(i32, i32)> = Vec::with_capacity(spans.len());
    for &s in spans.iter() {
        match out.last_mut() {
            Some(l) if s.0 <= l.1 => l.1 = l.1.max(s.1),
            _ => out.push(s),
        }
    }
    *spans = out;
}

impl Region {
    pub fn new() -> Region {
        Region { bands: Vec::new() }
    }

    pub fn from_rect(r: Rect) -> Region {
        if r.is_empty() {
            return Region::new();
        }
        Region { bands: alloc::vec![Band { y0: r.bottom, y1: r.top, spans: alloc::vec![(r.left, r.right)] }] }
    }

    pub fn from_rects(rs: &[Rect]) -> Region {
        let mut acc = Region::new();
        for r in rs {
            acc = acc.combine(&Region::from_rect(*r), Op::Or);
        }
        acc
    }

    /// From rows of spans (y -> [x0, x1) list, any order or overlap).
    pub fn from_rows(rows: BTreeMap<i32, Vec<(i32, i32)>>) -> Region {
        let mut g = Region::new();
        for (y, mut spans) in rows {
            normalise(&mut spans);
            if !spans.is_empty() {
                g.push_band(y, y + 1, spans);
            }
        }
        g
    }

    fn push_band(&mut self, y0: i32, y1: i32, spans: Vec<(i32, i32)>) {
        if let Some(last) = self.bands.last_mut() {
            if last.y1 == y0 && last.spans == spans {
                last.y1 = y1;
                return;
            }
        }
        self.bands.push(Band { y0, y1, spans });
    }

    pub fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    /// RGN_NULL, RGN_RECT or RGN_COMPLEX.
    pub fn complexity(&self) -> i32 {
        match self.bands.as_slice() {
            [] => RGN_NULL,
            [b] if b.spans.len() == 1 => RGN_RECT,
            _ => RGN_COMPLEX,
        }
    }

    pub fn combine(&self, o: &Region, op: Op) -> Region {
        let mut ys: Vec<i32> = Vec::with_capacity((self.bands.len() + o.bands.len()) * 2);
        for b in self.bands.iter().chain(o.bands.iter()) {
            ys.push(b.y0);
            ys.push(b.y1);
        }
        ys.sort_unstable();
        ys.dedup();
        let empty: Vec<(i32, i32)> = Vec::new();
        let (mut ia, mut ib) = (0, 0);
        let at = |g: &Region, i: &mut usize, y: i32| -> Option<usize> {
            while *i < g.bands.len() && g.bands[*i].y1 <= y {
                *i += 1;
            }
            (*i < g.bands.len() && g.bands[*i].y0 <= y).then_some(*i)
        };
        let mut out = Region::new();
        for w in ys.windows(2) {
            let (y0, y1) = (w[0], w[1]);
            let sa = at(self, &mut ia, y0).map_or(&empty, |i| &self.bands[i].spans);
            let sb = at(o, &mut ib, y0).map_or(&empty, |i| &o.bands[i].spans);
            let mut spans = Vec::new();
            combine_spans(sa, sb, op, &mut spans);
            if !spans.is_empty() {
                out.push_band(y0, y1, spans);
            }
        }
        out
    }

    pub fn intersect_rect(&self, r: Rect) -> Region {
        self.combine(&Region::from_rect(r), Op::And)
    }

    /// The region as disjoint rectangles, bottom band first.
    pub fn rects(&self) -> Vec<Rect> {
        let mut v = Vec::new();
        for b in &self.bands {
            for &(x0, x1) in &b.spans {
                v.push(Rect::new(x0, b.y0, x1, b.y1));
            }
        }
        v
    }

    pub fn bounds(&self) -> Rect {
        let (Some(first), Some(last)) = (self.bands.first(), self.bands.last()) else { return Rect::default() };
        let l = self.bands.iter().map(|b| b.spans[0].0).min().unwrap_or(0);
        let r = self.bands.iter().map(|b| b.spans[b.spans.len() - 1].1).max().unwrap_or(0);
        Rect::new(l, first.y0, r, last.y1)
    }

    pub fn contains(&self, p: Point) -> bool {
        self.bands.iter().any(|b| b.y0 <= p.y && p.y < b.y1 && b.spans.iter().any(|&(x0, x1)| x0 <= p.x && p.x < x1))
    }

    /// How much of `r` is inside: 0 none, 1 partly, 2 all.
    pub fn rect_inside(&self, r: Rect) -> i32 {
        let i = self.intersect_rect(r);
        if i.is_empty() {
            0
        } else if i == Region::from_rect(r) {
            2
        } else {
            1
        }
    }

    pub fn offset(&mut self, dx: i32, dy: i32) {
        for b in self.bands.iter_mut() {
            b.y0 += dy;
            b.y1 += dy;
            for s in b.spans.iter_mut() {
                s.0 += dx;
                s.1 += dx;
            }
        }
    }

    pub fn area(&self) -> i64 {
        self.bands.iter().map(|b| (b.y1 - b.y0) as i64 * b.spans.iter().map(|s| (s.1 - s.0) as i64).sum::<i64>()).sum()
    }
}

/// Accumulates pixel spans row by row for rasterisation.
#[derive(Default)]
pub struct Spans {
    rows: BTreeMap<i32, Vec<(i32, i32)>>,
}

impl Spans {
    pub fn new() -> Spans {
        Spans::default()
    }

    pub fn add(&mut self, y: i32, x0: i32, x1: i32) {
        if x0 < x1 {
            self.rows.entry(y).or_default().push((x0, x1));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn into_region(self) -> Region {
        Region::from_rows(self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops() {
        let a = Region::from_rect(Rect::new(0, 0, 10, 10));
        let b = Region::from_rect(Rect::new(5, 5, 15, 15));
        assert_eq!(a.combine(&b, Op::Or).area(), 175);
        assert_eq!(a.combine(&b, Op::And), Region::from_rect(Rect::new(5, 5, 10, 10)));
        assert_eq!(a.combine(&b, Op::Xor).area(), 150);
        assert_eq!(a.combine(&b, Op::Diff).area(), 75);
        assert_eq!(a.combine(&b, Op::Or).bounds(), Rect::new(0, 0, 15, 15));
        assert_eq!(a.combine(&b, Op::Or).complexity(), RGN_COMPLEX);
        assert_eq!(a.complexity(), RGN_RECT);
        assert!(a.combine(&b, Op::Diff).contains(Point::new(1, 9)));
        assert!(!a.combine(&b, Op::Diff).contains(Point::new(6, 6)));
        // Union of adjacent rectangles coalesces into one.
        let c = Region::from_rects(&[Rect::new(0, 0, 5, 5), Rect::new(5, 0, 10, 5), Rect::new(0, 5, 10, 8)]);
        assert_eq!(c, Region::from_rect(Rect::new(0, 0, 10, 8)));
        assert_eq!(c.rect_inside(Rect::new(1, 1, 3, 3)), 2);
        assert_eq!(c.rect_inside(Rect::new(8, 6, 13, 13)), 1);
        assert_eq!(c.rect_inside(Rect::new(20, 20, 30, 30)), 0);
    }

    #[test]
    fn rows_coalesce() {
        let mut s = Spans::new();
        for y in 0..4 {
            s.add(y, 2, 5);
            s.add(y, 4, 8);
        }
        let g = s.into_region();
        assert_eq!(g.rects(), alloc::vec![Rect::new(2, 0, 8, 4)]);
    }
}
