//! Points and rectangles in device coordinates.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Point { x, y }
    }
}

/// A rectangle like PM's RECTL: left and bottom inclusive, right and top
/// exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
    pub top: i32,
}

impl Rect {
    pub const fn new(left: i32, bottom: i32, right: i32, top: i32) -> Self {
        Rect { left, bottom, right, top }
    }

    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.top <= self.bottom
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.top - self.bottom
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect {
            left: self.left.max(o.left),
            bottom: self.bottom.max(o.bottom),
            right: self.right.min(o.right),
            top: self.top.min(o.top),
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.bottom && p.y < self.top
    }

    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect { left: self.left + dx, bottom: self.bottom + dy, right: self.right + dx, top: self.top + dy }
    }

    /// The parts of `self` not covered by `o` (at most four rectangles).
    pub fn subtract(&self, o: &Rect, out: &mut alloc::vec::Vec<Rect>) {
        let i = self.intersect(o);
        if i.is_empty() {
            out.push(*self);
            return;
        }
        if self.top > i.top {
            out.push(Rect::new(self.left, i.top, self.right, self.top));
        }
        if i.bottom > self.bottom {
            out.push(Rect::new(self.left, self.bottom, self.right, i.bottom));
        }
        if i.left > self.left {
            out.push(Rect::new(self.left, i.bottom, i.left, i.top));
        }
        if self.right > i.right {
            out.push(Rect::new(i.right, i.bottom, self.right, i.top));
        }
    }
}
