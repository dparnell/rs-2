//! Damaged areas of a surface: what was drawn since the list was last
//! taken, as a few rectangles, so that a display that must be told about
//! changes (VMware SVGA II, virtio-gpu, a shadow framebuffer) copies only
//! those.
//!
//! The list holds at most [`MAX`] rectangles. A new one that touches or
//! nearly touches one already there joins it (their bounding box) when
//! that adds little area; when the list is full it joins the one it adds
//! the least area to. The result covers everything drawn, with some
//! overdraw, and stays cheap to keep per pixel.

use crate::rect::Rect;

/// Rectangles kept at most.
pub const MAX: usize = 16;

/// Area (pixels) a merge may add beyond the two rectangles and still be
/// taken when the list is not full.
const SLACK: i64 = 64 * 64;

#[derive(Clone, Copy, Debug)]
pub struct Damage {
    rects: [Rect; MAX],
    n: usize,
    /// The rectangle last grown (checked first: drawing is local).
    last: usize,
}

impl Default for Damage {
    fn default() -> Self {
        Damage::new()
    }
}

fn area(r: &Rect) -> i64 {
    r.width() as i64 * r.height() as i64
}

fn union(a: &Rect, b: &Rect) -> Rect {
    Rect::new(a.left.min(b.left), a.bottom.min(b.bottom), a.right.max(b.right), a.top.max(b.top))
}

fn covers(a: &Rect, b: &Rect) -> bool {
    a.left <= b.left && a.bottom <= b.bottom && a.right >= b.right && a.top >= b.top
}

/// Area a merge of `a` and `b` would add beyond them.
fn waste(a: &Rect, b: &Rect) -> i64 {
    let i = a.intersect(b);
    let overlap = if i.is_empty() { 0 } else { area(&i) };
    area(&union(a, b)) - area(a) - area(b) + overlap
}

impl Damage {
    pub const fn new() -> Damage {
        Damage { rects: [Rect::new(0, 0, 0, 0); MAX], n: 0, last: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn rects(&self) -> &[Rect] {
        &self.rects[..self.n]
    }

    pub fn clear(&mut self) {
        self.n = 0;
        self.last = 0;
    }

    /// Note that pixel (`x`, `y`) was drawn.
    #[inline]
    pub fn add_point(&mut self, x: i32, y: i32) {
        if self.n > 0 {
            let l = &self.rects[self.last];
            if x >= l.left && x < l.right && y >= l.bottom && y < l.top {
                return;
            }
        }
        self.add(Rect::new(x, y, x + 1, y + 1));
    }

    /// Note that `r` was drawn.
    pub fn add(&mut self, r: Rect) {
        if r.is_empty() {
            return;
        }
        if self.n > 0 && covers(&self.rects[self.last], &r) {
            return;
        }
        if self.rects[..self.n].iter().any(|e| covers(e, &r)) {
            return;
        }
        // The cheapest rectangle to join.
        let mut best = None;
        for (i, e) in self.rects[..self.n].iter().enumerate() {
            let w = waste(e, &r);
            if best.is_none_or(|(_, bw)| w < bw) {
                best = Some((i, w));
            }
        }
        match best {
            Some((i, w)) if w <= SLACK || self.n == MAX => {
                self.rects[i] = union(&self.rects[i], &r);
                self.last = i;
                self.absorb(i);
            }
            _ => {
                self.rects[self.n] = r;
                self.last = self.n;
                self.n += 1;
            }
        }
    }

    /// Rectangle `i` grew: let it swallow others it now covers or nearly
    /// meets.
    fn absorb(&mut self, mut i: usize) {
        let mut j = 0;
        while j < self.n {
            if j != i {
                let (a, b) = (self.rects[i], self.rects[j]);
                if covers(&a, &b) || waste(&a, &b) <= SLACK {
                    self.rects[i] = union(&a, &b);
                    self.n -= 1;
                    self.rects[j] = self.rects[self.n];
                    if i == self.n {
                        i = j;
                    }
                    j = 0;
                    continue;
                }
            }
            j += 1;
        }
        self.last = i;
    }

    /// Add every rectangle of `o`.
    pub fn merge(&mut self, o: &Damage) {
        for r in o.rects() {
            self.add(*r);
        }
    }

    /// The bounding box of everything (empty if nothing).
    pub fn bounds(&self) -> Rect {
        self.rects().iter().fold(Rect::default(), |a, r| if a.is_empty() { *r } else { union(&a, r) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covered(d: &Damage, x: i32, y: i32) -> bool {
        d.rects().iter().any(|r| r.contains(crate::Point::new(x, y)))
    }

    #[test]
    fn points_grow_one_rect() {
        let mut d = Damage::new();
        for y in 10..20 {
            for x in 30..50 {
                d.add_point(x, y);
            }
        }
        assert_eq!(d.rects(), &[Rect::new(30, 10, 50, 20)]);
    }

    #[test]
    fn far_apart_stay_apart() {
        let mut d = Damage::new();
        d.add(Rect::new(0, 0, 100, 100));
        d.add(Rect::new(900, 600, 1000, 700));
        assert_eq!(d.rects().len(), 2);
        d.add(Rect::new(10, 10, 20, 20));
        assert_eq!(d.rects().len(), 2);
    }

    #[test]
    fn capped_and_complete() {
        let mut d = Damage::new();
        let mut all = alloc::vec::Vec::new();
        for i in 0..100 {
            let r = Rect::new((i * 97) % 1000, (i * 53) % 700, (i * 97) % 1000 + 5, (i * 53) % 700 + 3);
            all.push(r);
            d.add(r);
            assert!(d.rects().len() <= MAX);
        }
        for r in &all {
            assert!(covered(&d, r.left, r.bottom) && covered(&d, r.right - 1, r.top - 1));
        }
        // No rectangle covers another.
        for (i, a) in d.rects().iter().enumerate() {
            for (j, b) in d.rects().iter().enumerate() {
                assert!(i == j || !covers(a, b));
            }
        }
    }

    #[test]
    fn growing_rect_swallows_neighbours() {
        let mut d = Damage::new();
        d.add(Rect::new(0, 0, 10, 10));
        d.add(Rect::new(500, 0, 510, 10));
        d.add(Rect::new(0, 0, 600, 20));
        assert_eq!(d.rects(), &[Rect::new(0, 0, 600, 20)]);
    }
}
