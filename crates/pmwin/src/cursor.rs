//! The text cursor (WinCreateCursor, WinShowCursor, WinDestroyCursor,
//! WinQueryCursorInfo): one for the whole system, owned by a window.
//!
//! As in OS/2's PMWIN, a cursor is created hidden (its hide level is 1;
//! WinShowCursor TRUE brings it to 0), it is drawn by inverting its
//! rectangle, and it blinks while visible when it has CURSOR_FLASH: the
//! window gets WM_TIMER TID_CURSOR at the cursor blink rate and
//! WinDefWindowProc flips it. WinBeginPaint hides it while its window is
//! painted, WinEndPaint shows it again, and destroying the window destroys
//! it. The drawing itself is the caller's: `invert` inverts the cursor's
//! rectangles ([`Cursor::rects`]) on the screen and says whether it could
//! (false while the window is not showing).

use alloc::vec::Vec;
use gpi::Rect;

pub const CURSOR_SOLID: u32 = 0x0000;
pub const CURSOR_HALFTONE: u32 = 0x0001;
pub const CURSOR_FRAME: u32 = 0x0002;
pub const CURSOR_FLASH: u32 = 0x0004;
pub const CURSOR_BIDI_FIRST: u32 = 0x0100;
pub const CURSOR_BIDI_LAST: u32 = 0x0200;
pub const CURSOR_SETPOS: u32 = 0x8000;
/// Flags WinCreateCursor accepts (others give PMERR_INVALID_FLAG); 0x10 is
/// reserved but allowed.
const VALID: u32 = 0x8317;

/// The system timer that blinks the cursor.
pub const TID_CURSOR: u32 = 0xffff;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub hwnd: u32,
    pub x: i32,
    pub y: i32,
    pub cx: i32,
    pub cy: i32,
    pub fs: u32,
    /// Clipping rectangle in window coordinates (empty: the window).
    pub clip: Rect,
    /// Hide level: shown at 0.
    pub level: i32,
    /// Blink phase: the cursor is drawn while on (and level is 0).
    pub on: bool,
    /// Currently inverted on the screen.
    pub visible: bool,
}

/// What WinCreateCursor did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Created {
    /// Invalid flags (PMERR_INVALID_FLAG).
    InvalidFlag,
    /// CURSOR_SETPOS for a window that does not own the cursor.
    NotOwner,
    /// Nothing changed (or only the clipping rectangle).
    Unchanged,
    /// Moved (CURSOR_SETPOS).
    Moved,
    /// A new cursor; the previous one (a window, 0: none) was destroyed.
    /// Its blink timer must be stopped and, with `flash`, the new one's
    /// started.
    New { previous: u32, flash: bool },
}

#[derive(Default)]
pub struct CursorState {
    pub cur: Option<Cursor>,
}

impl Cursor {
    /// The rectangles to invert, in window coordinates, before clipping:
    /// a border `cx` x `cy` thick inside the cursor's rectangle, so the
    /// whole rectangle for a solid cursor and `border` thick for
    /// CURSOR_FRAME; a bidirectional cursor has a flag 5 pixels wide on
    /// its leading side.
    pub fn rects(&self, border: (i32, i32)) -> Vec<Rect> {
        let (bx, by) = if self.fs & CURSOR_FRAME != 0 { border } else { (self.cx, self.cy) };
        let mut out = Vec::new();
        frame(&mut out, Rect::new(self.x, self.y, self.x + self.cx, self.y + self.cy), bx, by);
        if self.fs & (CURSOR_BIDI_FIRST | CURSOR_BIDI_LAST) != 0 && self.fs & (CURSOR_HALFTONE | CURSOR_FRAME) == 0 {
            let (l, r) = if self.fs & 0x300 == 0x300 { (self.x - 5, self.x) } else { (self.x + self.cx, self.x + self.cx + 5) };
            frame(&mut out, Rect::new(l, self.y, r, self.y + self.cx + 1), bx, by);
        }
        out
    }

    /// The rectangle drawing is clipped to, in window coordinates.
    pub fn clip_rect(&self, window: Rect) -> Rect {
        if self.clip.is_empty() { window } else { self.clip }
    }
}

/// The parts of `r` within `bx`/`by` of its edges (WinDrawBorder's
/// border), without overlaps.
fn frame(out: &mut Vec<Rect>, r: Rect, bx: i32, by: i32) {
    if r.is_empty() {
        return;
    }
    if bx * 2 >= r.width() || by * 2 >= r.height() {
        out.push(r);
        return;
    }
    out.push(Rect::new(r.left, r.bottom, r.right, r.bottom + by));
    out.push(Rect::new(r.left, r.top - by, r.right, r.top));
    out.push(Rect::new(r.left, r.bottom + by, r.left + bx, r.top - by));
    out.push(Rect::new(r.right - bx, r.bottom + by, r.right, r.top - by));
}

impl CursorState {
    /// WinCreateCursor(hwnd, x, y, cx, cy, fs, prclClip); `size` is the
    /// default size for cx or cy 0.
    #[allow(clippy::too_many_arguments)]
    pub fn create(&mut self, hwnd: u32, x: i32, y: i32, cx: i32, cy: i32, fs: u32, clip: Option<Rect>, size: (i32, i32), invert: &mut dyn FnMut(&Cursor) -> bool) -> Created {
        if fs & !VALID != 0 {
            return Created::InvalidFlag;
        }
        let mut previous = 0;
        if fs & CURSOR_SETPOS == 0 {
            let cx = if cx == 0 { size.0 } else { cx };
            let cy = if cy == 0 { size.1 } else { cy };
            if let (Some(c), Some(clip)) = (&self.cur, clip) {
                if c.hwnd == hwnd && c.fs == fs && c.x == x && c.y == y && c.cx == cx && c.cy == cy && c.clip == clip {
                    return Created::Unchanged;
                }
            }
            if let Some(old) = self.cur.as_ref().map(|c| c.hwnd) {
                self.destroy(old, invert);
                previous = old;
            }
            self.cur = Some(Cursor { hwnd, x, y, cx, cy, fs, clip: Rect::default(), level: 1, on: true, visible: false });
        } else {
            let Some(c) = self.cur.as_mut().filter(|c| c.hwnd == hwnd) else { return Created::NotOwner };
            if c.x == x && c.y == y {
                if let Some(r) = clip {
                    c.clip = r;
                }
                return Created::Unchanged;
            }
            if c.visible {
                c.visible = !invert(c);
            }
        }
        let c = self.cur.as_mut().unwrap();
        c.clip = clip.unwrap_or_default();
        c.x = x;
        c.y = y;
        if fs & CURSOR_SETPOS == 0 {
            c.on = true;
        }
        c.visible = c.level == 0 && invert(c);
        if fs & CURSOR_SETPOS == 0 { Created::New { previous, flash: fs & CURSOR_FLASH != 0 } } else { Created::Moved }
    }

    /// WinShowCursor(hwnd, fShow): false if hwnd has no cursor (or showing
    /// a cursor that is not hidden).
    pub fn show(&mut self, hwnd: u32, show: bool, invert: &mut dyn FnMut(&Cursor) -> bool) -> bool {
        let Some(c) = self.cur.as_mut().filter(|c| c.hwnd == hwnd) else { return false };
        if !show {
            if c.visible {
                c.visible = !invert(c);
            }
            c.level += 1;
            true
        } else if c.level > 0 {
            c.level -= 1;
            if c.level == 0 {
                c.visible = c.on && invert(c);
            }
            true
        } else {
            false
        }
    }

    /// WinDestroyCursor(hwnd): false if hwnd has no cursor. The caller
    /// stops the blink timer.
    pub fn destroy(&mut self, hwnd: u32, invert: &mut dyn FnMut(&Cursor) -> bool) -> bool {
        if !self.cur.as_ref().is_some_and(|c| c.hwnd == hwnd) {
            return false;
        }
        self.show(hwnd, false, invert);
        self.cur = None;
        true
    }

    /// WM_TIMER TID_CURSOR reached WinDefWindowProc of hwnd.
    pub fn blink(&mut self, hwnd: u32, invert: &mut dyn FnMut(&Cursor) -> bool) {
        let Some(c) = self.cur.as_mut().filter(|c| c.hwnd == hwnd) else { return };
        c.on = !c.on;
        if c.level == 0 {
            c.visible ^= invert(c);
        }
    }

    /// The window owning the cursor (0: none).
    pub fn owner(&self) -> u32 {
        self.cur.as_ref().map_or(0, |c| c.hwnd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(n: &mut u32) -> impl FnMut(&Cursor) -> bool + '_ {
        move |_| {
            *n += 1;
            true
        }
    }

    #[test]
    fn created_hidden_then_shown() {
        let mut s = CursorState::default();
        let mut n = 0;
        let r = s.create(5, 10, 20, 0, 16, CURSOR_FLASH, None, (2, 2), &mut count(&mut n));
        assert_eq!(r, Created::New { previous: 0, flash: true });
        let c = s.cur.clone().unwrap();
        assert_eq!((c.cx, c.cy, c.level, c.visible), (2, 16, 1, false));
        assert_eq!(n, 0);
        assert!(s.show(5, true, &mut count(&mut n)));
        assert_eq!(n, 1);
        assert!(s.cur.as_ref().unwrap().visible);
        // Not hidden any more: a second show fails.
        assert!(!s.show(5, true, &mut count(&mut n)));
        // Another window does not own it.
        assert!(!s.show(6, false, &mut count(&mut n)));
        // Hiding twice needs two shows.
        assert!(s.show(5, false, &mut count(&mut n)));
        assert!(s.show(5, false, &mut count(&mut n)));
        assert_eq!(n, 2);
        assert!(s.show(5, true, &mut count(&mut n)));
        assert_eq!(n, 2);
        assert!(s.show(5, true, &mut count(&mut n)));
        assert_eq!(n, 3);
    }

    #[test]
    fn blink_and_setpos() {
        let mut s = CursorState::default();
        let mut n = 0;
        s.create(5, 0, 0, 2, 10, CURSOR_FLASH, None, (1, 1), &mut count(&mut n));
        s.show(5, true, &mut count(&mut n));
        s.blink(5, &mut count(&mut n));
        let c = s.cur.clone().unwrap();
        assert!(!c.on && !c.visible);
        // Moving while blinked off draws it at the new place (OS/2 does).
        assert_eq!(s.create(5, 8, 0, 0, 0, CURSOR_SETPOS, None, (1, 1), &mut count(&mut n)), Created::Moved);
        assert!(s.cur.as_ref().unwrap().visible);
        assert_eq!(n, 3);
        // Moving while visible: erased, then drawn.
        s.create(5, 9, 0, 0, 0, CURSOR_SETPOS, None, (1, 1), &mut count(&mut n));
        assert_eq!(n, 5);
        assert_eq!(s.create(5, 9, 0, 0, 0, CURSOR_SETPOS, None, (1, 1), &mut count(&mut n)), Created::Unchanged);
        assert_eq!(s.create(6, 1, 1, 0, 0, CURSOR_SETPOS, None, (1, 1), &mut count(&mut n)), Created::NotOwner);
        assert_eq!(s.create(6, 1, 1, 0, 0, 0x40, None, (1, 1), &mut count(&mut n)), Created::InvalidFlag);
    }

    #[test]
    fn new_cursor_replaces_and_destroy() {
        let mut s = CursorState::default();
        let mut n = 0;
        s.create(5, 0, 0, 2, 10, 0, None, (1, 1), &mut count(&mut n));
        s.show(5, true, &mut count(&mut n));
        let r = s.create(7, 3, 3, 2, 10, 0, Some(Rect::new(0, 0, 50, 50)), (1, 1), &mut count(&mut n));
        assert_eq!(r, Created::New { previous: 5, flash: false });
        // The old one was erased.
        assert_eq!(n, 2);
        assert_eq!(s.owner(), 7);
        assert_eq!(s.cur.as_ref().unwrap().clip, Rect::new(0, 0, 50, 50));
        // Same again with the same clip rectangle: nothing happens.
        assert_eq!(s.create(7, 3, 3, 2, 10, 0, Some(Rect::new(0, 0, 50, 50)), (1, 1), &mut count(&mut n)), Created::Unchanged);
        assert!(!s.destroy(5, &mut count(&mut n)));
        assert!(s.destroy(7, &mut count(&mut n)));
        assert_eq!(s.owner(), 0);
    }

    #[test]
    fn shapes() {
        let c = Cursor { x: 10, y: 20, cx: 2, cy: 16, ..Default::default() };
        assert_eq!(c.rects((1, 1)), [Rect::new(10, 20, 12, 36)]);
        let f = Cursor { x: 0, y: 0, cx: 8, cy: 16, fs: CURSOR_FRAME, ..Default::default() };
        let r = f.rects((1, 1));
        assert_eq!(r.len(), 4);
        let area: i32 = r.iter().map(|r| r.width() * r.height()).sum();
        assert_eq!(area, 8 * 16 - 6 * 14);
        let b = Cursor { x: 10, y: 0, cx: 2, cy: 16, fs: CURSOR_BIDI_LAST, ..Default::default() };
        assert_eq!(b.rects((1, 1))[1], Rect::new(12, 0, 17, 3));
        assert_eq!(c.clip_rect(Rect::new(0, 0, 100, 50)), Rect::new(0, 0, 100, 50));
    }
}
