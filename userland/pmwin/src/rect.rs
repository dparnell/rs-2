//! Rectangle helpers (WinSetRect, WinIntersectRect, ...): RECTL is
//! left, bottom, right, top; right and top are exclusive.

#![allow(non_snake_case)]

use crate::gpiapi::{PointL, RectL};

fn rd(p: *const RectL) -> Option<RectL> {
    (!p.is_null()).then(|| unsafe { p.read_unaligned() })
}

fn wr(p: *mut RectL, r: RectL) -> u32 {
    if p.is_null() {
        return 0;
    }
    unsafe { p.write_unaligned(r) };
    1
}

fn empty(r: &RectL) -> bool {
    r.left >= r.right || r.bottom >= r.top
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetRect(_hab: u32, p: *mut RectL, l: i32, b: i32, r: i32, t: i32) -> u32 {
    wr(p, RectL { left: l, bottom: b, right: r, top: t })
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetRectEmpty(_hab: u32, p: *mut RectL) -> u32 {
    wr(p, RectL { left: 0, bottom: 0, right: 0, top: 0 })
}

#[unsafe(no_mangle)]
pub extern "C" fn WinCopyRect(_hab: u32, dst: *mut RectL, src: *const RectL) -> u32 {
    match rd(src) {
        Some(r) => wr(dst, r),
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinIsRectEmpty(_hab: u32, p: *const RectL) -> u32 {
    rd(p).is_none_or(|r| empty(&r)) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinEqualRect(_hab: u32, a: *const RectL, b: *const RectL) -> u32 {
    match (rd(a), rd(b)) {
        (Some(a), Some(b)) => (a.left == b.left && a.bottom == b.bottom && a.right == b.right && a.top == b.top) as u32,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinOffsetRect(_hab: u32, p: *mut RectL, dx: i32, dy: i32) -> u32 {
    let Some(r) = rd(p) else { return 0 };
    wr(p, RectL { left: r.left + dx, bottom: r.bottom + dy, right: r.right + dx, top: r.top + dy })
}

#[unsafe(no_mangle)]
pub extern "C" fn WinInflateRect(_hab: u32, p: *mut RectL, dx: i32, dy: i32) -> u32 {
    let Some(r) = rd(p) else { return 0 };
    wr(p, RectL { left: r.left - dx, bottom: r.bottom - dy, right: r.right + dx, top: r.top + dy })
}

/// WinPtInRect(hab, prcl, pptl)
#[unsafe(no_mangle)]
pub extern "C" fn WinPtInRect(_hab: u32, p: *const RectL, pt: *const PointL) -> u32 {
    let (Some(r), false) = (rd(p), pt.is_null()) else { return 0 };
    let q = unsafe { pt.read_unaligned() };
    (q.x >= r.left && q.x < r.right && q.y >= r.bottom && q.y < r.top) as u32
}

/// WinIntersectRect(hab, prclDst, prclSrc1, prclSrc2): FALSE (and an
/// empty result) when they do not overlap.
#[unsafe(no_mangle)]
pub extern "C" fn WinIntersectRect(_hab: u32, dst: *mut RectL, a: *const RectL, b: *const RectL) -> u32 {
    let (Some(a), Some(b)) = (rd(a), rd(b)) else { return 0 };
    let r = RectL { left: a.left.max(b.left), bottom: a.bottom.max(b.bottom), right: a.right.min(b.right), top: a.top.min(b.top) };
    if empty(&r) {
        wr(dst, RectL { left: 0, bottom: 0, right: 0, top: 0 });
        return 0;
    }
    wr(dst, r)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinUnionRect(_hab: u32, dst: *mut RectL, a: *const RectL, b: *const RectL) -> u32 {
    let (Some(a), Some(b)) = (rd(a), rd(b)) else { return 0 };
    let r = match (empty(&a), empty(&b)) {
        (true, true) => RectL { left: 0, bottom: 0, right: 0, top: 0 },
        (true, false) => b,
        (false, true) => a,
        _ => RectL { left: a.left.min(b.left), bottom: a.bottom.min(b.bottom), right: a.right.max(b.right), top: a.top.max(b.top) },
    };
    wr(dst, r);
    (!empty(&r)) as u32
}

/// WinSubtractRect: src1 minus src2 when the rest is a rectangle, else
/// src1 itself.
#[unsafe(no_mangle)]
pub extern "C" fn WinSubtractRect(_hab: u32, dst: *mut RectL, a: *const RectL, b: *const RectL) -> u32 {
    let (Some(a), Some(b)) = (rd(a), rd(b)) else { return 0 };
    let mut r = a;
    if b.bottom <= a.bottom && b.top >= a.top {
        if b.left <= a.left && b.right > a.left {
            r.left = b.right.min(a.right);
        } else if b.right >= a.right && b.left < a.right {
            r.right = b.left.max(a.left);
        }
    } else if b.left <= a.left && b.right >= a.right {
        if b.bottom <= a.bottom && b.top > a.bottom {
            r.bottom = b.top.min(a.top);
        } else if b.top >= a.top && b.bottom < a.top {
            r.top = b.bottom.max(a.bottom);
        }
    }
    if empty(&r) {
        r = RectL { left: 0, bottom: 0, right: 0, top: 0 };
    }
    wr(dst, r);
    (!empty(&r)) as u32
}
