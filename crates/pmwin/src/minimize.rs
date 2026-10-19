//! Where minimized frames go: icons in rows along the bottom of the
//! desktop, left to right, the next row above when one is full (classic
//! PM "minimize to desktop").

use gpi::Rect;

/// Size of a minimized frame: the icon with its title below.
pub const ICON_W: i32 = 84;
pub const ICON_H: i32 = 62;
/// Gap between icons and to the screen edge.
pub const GAP: i32 = 4;

/// The slot rectangle number `n` on a `w` x `h` desktop.
pub fn slot(n: usize, w: i32, h: i32) -> Rect {
    let per_row = ((w - GAP) / (ICON_W + GAP)).max(1) as usize;
    let (row, col) = ((n / per_row) as i32, (n % per_row) as i32);
    let x = GAP + col * (ICON_W + GAP);
    let y = (GAP + row * (ICON_H + GAP)).min((h - ICON_H).max(0));
    Rect::new(x, y, x + ICON_W, y + ICON_H)
}

/// The first slot that does not overlap any of `taken`.
pub fn free_slot(taken: &[Rect], w: i32, h: i32) -> Rect {
    for n in 0..1000 {
        let r = slot(n, w, h);
        if !taken.iter().any(|t| !t.intersect(&r).is_empty()) {
            return r;
        }
    }
    slot(0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_fill_rows() {
        let a = free_slot(&[], 1024, 768);
        assert_eq!((a.left, a.bottom), (GAP, GAP));
        let b = free_slot(&[a], 1024, 768);
        assert_eq!((b.left, b.bottom), (GAP + ICON_W + GAP, GAP));
        // A moved icon somewhere else does not block the first slot.
        let moved = Rect::new(500, 500, 500 + ICON_W, 500 + ICON_H);
        assert_eq!(free_slot(&[moved, b], 1024, 768), a);
        // A full row continues above.
        let per_row = ((1024 - GAP) / (ICON_W + GAP)) as usize;
        let taken: alloc::vec::Vec<Rect> = (0..per_row).map(|n| slot(n, 1024, 768)).collect();
        let c = free_slot(&taken, 1024, 768);
        assert_eq!((c.left, c.bottom), (GAP, GAP + ICON_H + GAP));
    }
}
