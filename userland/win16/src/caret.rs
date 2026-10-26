//! The caret (CreateCaret, SetCaretPos, ShowCaret...) on PM's text
//! cursor of the window's client, and the clipboard (OpenClipboard,
//! SetClipboardData...) on PM's: CF_TEXT is PM's CF_TEXT, given to PM in
//! giveable shared memory and read back into a global block.

use alloc::vec::Vec;
use thunk16::Pascal;

use crate::kernel::TASK;
use crate::pm::*;
use crate::user::{self, wnd};
use crate::Global;

struct Caret {
    hwnd: u16,
    cx: i32,
    cy: i32,
    x: i32,
    y: i32,
    /// Hidden this many times (ShowCaret undoes one).
    hidden: i32,
}

static CARET: Global<Option<Caret>> = Global::new(None);
static BLINK: Global<u16> = Global::new(530);

fn pm_y(h: u16, y: i32, cy: i32) -> i32 {
    let c = wnd(h).map_or(0, |w| w.client);
    user::client_height(c) - y - cy
}

pub fn create(h: u16, gray: bool, cx: i32, cy: i32) {
    destroy();
    let Some(c) = wnd(h).map(|w| w.client) else { return };
    let cx = if cx <= 0 { 1 } else { cx };
    let cy = if cy <= 0 { 16 } else { cy };
    // CURSOR_FLASH; CURSOR_HALFTONE for a gray caret.
    let fs = 0x0004 | if gray { 0x0001 } else { 0 };
    unsafe { WinCreateCursor(c, 0, pm_y(h, 0, cy), cx, cy, fs, core::ptr::null()) };
    *CARET.get() = Some(Caret { hwnd: h, cx, cy, x: 0, y: 0, hidden: 1 });
}

pub fn destroy() {
    if let Some(c) = CARET.get().take() {
        if let Some(pm) = wnd(c.hwnd).map(|w| w.client) {
            unsafe { WinDestroyCursor(pm) };
        }
    }
}

pub fn set_pos(x: i32, y: i32) {
    let Some(c) = CARET.get().as_mut() else { return };
    c.x = x;
    c.y = y;
    let (h, cy) = (c.hwnd, c.cy);
    let Some(pm) = wnd(h).map(|w| w.client) else { return };
    // CURSOR_SETPOS
    unsafe { WinCreateCursor(pm, x, pm_y(h, y, cy), 0, 0, 0x8000, core::ptr::null()) };
}

pub fn show(h: u16, on: bool) {
    let Some(c) = CARET.get().as_mut() else { return };
    if h != 0 && c.hwnd != h {
        return;
    }
    let was = c.hidden <= 0;
    c.hidden += if on { -1 } else { 1 };
    c.hidden = c.hidden.max(0);
    let now = c.hidden <= 0;
    if was != now {
        if let Some(pm) = wnd(c.hwnd).map(|w| w.client) {
            unsafe { WinShowCursor(pm, now as u32) };
        }
    }
}

/// BeginPaint: PM hides the cursor while its window paints.
pub fn hide_for_paint(_h: u16) {}

/// A window is going: its caret too.
pub fn window_gone(h: u16) {
    if CARET.get().as_ref().is_some_and(|c| c.hwnd == h) {
        destroy();
    }
}

// ---- The clipboard ----


/// Text on the clipboard (CF_TEXT).
pub fn clip_text() -> Option<Vec<u8>> {
    let hab = TASK.get().hab;
    let opened = unsafe { WinOpenClipbrd(hab) } != 0;
    let p = unsafe { WinQueryClipbrdData(hab, 1) } as *const u8;
    let r = if p.is_null() { None } else { Some(unsafe { crate::cstr(p) }.to_vec()) };
    if opened {
        unsafe { WinCloseClipbrd(hab) };
    }
    r
}

/// Put text on the clipboard (replacing what is there).
pub fn set_clip_text(t: &[u8]) -> bool {
    let hab = TASK.get().hab;
    if unsafe { WinOpenClipbrd(hab) } == 0 {
        return false;
    }
    unsafe { WinEmptyClipbrd(hab) };
    let ok = give_text(t);
    unsafe { WinCloseClipbrd(hab) };
    ok
}

fn give_text(t: &[u8]) -> bool {
    let mut p: *mut u8 = core::ptr::null_mut();
    if unsafe { os2::DosAllocSharedMem(&mut p, core::ptr::null(), t.len() as u32 + 1, os2::PAG_READ | os2::PAG_WRITE | os2::PAG_COMMIT | os2::OBJ_GIVEABLE) } != 0 {
        return false;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(t.as_ptr(), p, t.len());
        *p.add(t.len()) = 0;
        // CFI_POINTER
        WinSetClipbrdData(TASK.get().hab, p as u32, 1, 0x0400) != 0
    }
}

pub fn call(ord: u32, a: &mut Pascal) -> Option<u32> {
    Some(match ord {
        // CreateCaret(hwnd, hbitmap, cx, cy)
        163 => {
            let (h, bm, cx, cy) = (a.w(), a.w(), a.i(), a.i());
            create(h, bm == 1, cx, cy);
            0
        }
        // DestroyCaret
        164 => {
            destroy();
            0
        }
        // SetCaretPos(x, y)
        165 => {
            let (x, y) = (a.i(), a.i());
            set_pos(x, y);
            0
        }
        // HideCaret(hwnd), ShowCaret(hwnd)
        166 | 167 => {
            let h = a.w();
            show(h, ord == 167);
            0
        }
        // SetCaretBlinkTime(ms), GetCaretBlinkTime
        168 => {
            *BLINK.get() = a.w();
            0
        }
        169 => *BLINK.get() as u32,
        // GetCaretPos(lpPoint)
        183 => {
            let p = a.p() as *mut i16;
            if let (Some(c), false) = (CARET.get().as_ref(), p.is_null()) {
                unsafe {
                    p.write_unaligned(c.x as i16);
                    p.add(1).write_unaligned(c.y as i16);
                }
            }
            0
        }
        _ => return None,
    })
}
