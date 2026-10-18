//! Regions as lists of non-overlapping rectangles.

use alloc::vec::Vec;

use gpi::Rect;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Region {
    pub rects: Vec<Rect>,
}

impl Region {
    pub fn new() -> Region {
        Region { rects: Vec::new() }
    }

    pub fn from_rect(r: Rect) -> Region {
        let mut g = Region::new();
        if !r.is_empty() {
            g.rects.push(r);
        }
        g
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Remove `r` from the region.
    pub fn subtract(&mut self, r: &Rect) {
        let mut out = Vec::new();
        for x in &self.rects {
            x.subtract(r, &mut out);
        }
        out.retain(|x| !x.is_empty());
        self.rects = out;
    }

    /// Keep only the parts inside `r`.
    pub fn intersect(&mut self, r: &Rect) {
        self.rects = self.rects.iter().map(|x| x.intersect(r)).filter(|x| !x.is_empty()).collect();
    }

    /// Add `r` (kept non-overlapping).
    pub fn add(&mut self, r: Rect) {
        if r.is_empty() {
            return;
        }
        let mut pieces = alloc::vec![r];
        for x in &self.rects {
            let mut next = Vec::new();
            for p in &pieces {
                p.subtract(x, &mut next);
            }
            pieces = next.into_iter().filter(|p| !p.is_empty()).collect();
        }
        self.rects.extend(pieces);
    }

    pub fn offset(&mut self, dx: i32, dy: i32) {
        for r in self.rects.iter_mut() {
            *r = r.offset(dx, dy);
        }
    }

    /// Smallest rectangle containing the region.
    pub fn bounds(&self) -> Rect {
        let mut it = self.rects.iter();
        let Some(&first) = it.next() else { return Rect::default() };
        it.fold(first, |a, r| Rect::new(a.left.min(r.left), a.bottom.min(r.bottom), a.right.max(r.right), a.top.max(r.top)))
    }

    pub fn area(&self) -> i64 {
        self.rects.iter().map(|r| r.width() as i64 * r.height() as i64).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtract_hole() {
        let mut g = Region::from_rect(Rect::new(0, 0, 10, 10));
        g.subtract(&Rect::new(2, 2, 4, 4));
        assert_eq!(g.area(), 96);
        assert!(g.rects.iter().all(|r| r.intersect(&Rect::new(2, 2, 4, 4)).is_empty()));
    }

    #[test]
    fn add_overlapping() {
        let mut g = Region::from_rect(Rect::new(0, 0, 10, 10));
        g.add(Rect::new(5, 5, 15, 15));
        assert_eq!(g.area(), 175);
        assert_eq!(g.bounds(), Rect::new(0, 0, 15, 15));
    }
}
