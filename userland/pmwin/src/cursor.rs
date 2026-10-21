//! The text cursor: WinCreateCursor, WinShowCursor, WinDestroyCursor,
//! WinQueryCursorInfo. The rules are `pmwin::cursor`'s; this draws it
//! (inverting its rectangles on the screen, within the window's visible
//! region) and runs its blink timer.

use pmwin::cursor::{Created, Cursor, CURSOR_HALFTONE, TID_CURSOR};
use pmwin::ws;

use crate::gpiapi::RectL;
use crate::misc::set_error;
use crate::state::{self, st, State};

const PMERR_INVALID_FLAG: u32 = 0x1019;

/// Invert the cursor on the screen; false if its window is not showing.
fn invert(s: &mut State, c: &Cursor) -> bool {
    let Some(w) = s.desk.get(c.hwnd) else { return false };
    if !s.desk.is_showing(c.hwnd) {
        return false;
    }
    let clip = c.clip_rect(w.rect());
    let vis = s.desk.visible_region(c.hwnd, w.style & ws::CLIPCHILDREN != 0);
    let o = s.desk.origin(c.hwnd);
    let border = (crate::misc::WinQuerySysValue(1, 26), crate::misc::WinQuerySysValue(1, 27));
    let half = gpi::pattern::standard(17).unwrap_or([0xff; 8]);
    let halftone = c.fs & CURSOR_HALFTONE != 0;
    crate::pointer::hide(s);
    let mut surface = s.surface;
    for r in c.rects(border) {
        let r = r.intersect(&clip).offset(o.x, o.y);
        for v in &vis.rects {
            surface.map(r.intersect(v), |x, y, p| if !halftone || gpi::pattern::bit(&half, x, y, (0, 0)) { p ^ 0xff_ffff } else { p });
        }
    }
    crate::pointer::show(s);
    true
}

/// Run `f` on the cursor state with the drawing callback.
fn with<R>(f: impl FnOnce(&mut pmwin::cursor::CursorState, &mut dyn FnMut(&Cursor) -> bool) -> R) -> R {
    let s = st();
    let mut cs = core::mem::take(&mut s.cursor);
    let r = f(&mut cs, &mut |c| invert(st(), c));
    st().cursor = cs;
    r
}

fn stop_timer(s: &mut State, h: u32) {
    s.timers.retain(|t| !(t.hwnd == h && t.id == TID_CURSOR));
}

/// WinCreateCursor(hwnd, x, y, cx, cy, fs, prclClip)
#[unsafe(no_mangle)]
pub extern "C" fn WinCreateCursor(h: u32, x: i32, y: i32, cx: i32, cy: i32, fs: u32, clip: *const RectL) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    let clip = (!clip.is_null()).then(|| unsafe { clip.read_unaligned() }.rect());
    // The default size is the border width (PM_ControlPanel CursorWidth
    // and CursorHeight in OS2.INI on OS/2).
    let size = (crate::misc::WinQuerySysValue(1, 26), crate::misc::WinQuerySysValue(1, 27));
    let r = with(|c, inv| c.create(h, x, y, cx, cy, fs, clip, size, inv));
    match r {
        Created::InvalidFlag => {
            set_error(PMERR_INVALID_FLAG);
            0
        }
        Created::NotOwner => 0,
        Created::Unchanged | Created::Moved => 1,
        Created::New { previous, flash } => {
            let s = st();
            if previous != 0 {
                stop_timer(s, previous);
            }
            stop_timer(s, h);
            if flash {
                let queue = s.queue_of(h);
                let ms = crate::misc::WinQuerySysValue(1, 9) as u32;
                s.timers.push(state::Timer { hwnd: h, queue, id: TID_CURSOR, interval: ms, next: state::now().wrapping_add(ms) });
                s.wake.insert(queue);
            }
            1
        }
    }
}

/// WinShowCursor(hwnd, fShow)
#[unsafe(no_mangle)]
pub extern "C" fn WinShowCursor(h: u32, show: u32) -> u32 {
    with(|c, inv| c.show(h, show != 0, inv)) as u32
}

/// WinDestroyCursor(hwnd)
#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyCursor(h: u32) -> u32 {
    let ok = with(|c, inv| c.destroy(h, inv));
    if ok {
        stop_timer(st(), h);
    }
    ok as u32
}

/// WinQueryCursorInfo(hwndDesktop, pCursorInfo): CURSORINFO (hwnd, x, y,
/// cx, cy, fs, rclClip); false without a cursor.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryCursorInfo(_desk: u32, p: *mut u32) -> u32 {
    let Some(c) = st().cursor.cur.clone() else { return 0 };
    if p.is_null() {
        return 0;
    }
    let v = [c.hwnd, c.x as u32, c.y as u32, c.cx as u32, c.cy as u32, c.fs, c.clip.left as u32, c.clip.bottom as u32, c.clip.right as u32, c.clip.top as u32];
    for (i, x) in v.iter().enumerate() {
        unsafe { p.add(i).write_unaligned(*x) };
    }
    1
}

/// WM_TIMER TID_CURSOR in WinDefWindowProc.
pub fn blink(h: u32) {
    with(|c, inv| c.blink(h, inv));
}

/// WinBeginPaint (hide) and WinEndPaint (show) of the cursor's window.
pub fn paint(h: u32, show: bool) {
    if h != 0 && st().cursor.owner() == h {
        with(|c, inv| c.show(h, show, inv));
    }
}

/// Windows being destroyed: the cursor goes with its window (nothing to
/// erase, the window's area is repainted).
pub fn windows_gone(s: &mut State, gone: &[u32]) {
    let owner = s.cursor.owner();
    if owner != 0 && gone.contains(&owner) {
        s.cursor.cur = None;
        stop_timer(s, owner);
    }
}
