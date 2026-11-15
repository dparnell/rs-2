//! What a window's move, hiding or destruction makes other windows
//! repaint, as IBM's PM: only what it uncovers, never a window above it
//! (Open Watcom's Dialog Editor read stale data when its tool palette,
//! nowhere near a control being sized, got a WM_PAINT).

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::check;
use crate::pm::*;

unsafe extern "C" {
    #[link_name = "PMWIN.703"]
    fn WinBeginPaint(h: u32, hps: u32, rcl: *mut i32) -> u32;
    #[link_name = "PMWIN.738"]
    fn WinEndPaint(hps: u32) -> u32;
}

/// WM_PAINTs per window (index by the window's id), and the union of the
/// rectangles painted (xLeft, yBottom, xRight, yTop) of the low window.
static PAINTS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static LOW_RECT: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];

extern "C" fn paint_proc(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg == 0x0023 {
        let id = unsafe { WinQueryWindowUShort(h, -1) } as usize & 3;
        PAINTS[id].fetch_add(1, Ordering::Relaxed);
        let mut r = [0i32; 4];
        let hps = unsafe { WinBeginPaint(h, 0, r.as_mut_ptr()) };
        if id == 1 {
            for i in 0..4 {
                LOW_RECT[i].store(r[i], Ordering::Relaxed);
            }
        }
        unsafe { WinEndPaint(hps) };
        return 0;
    }
    unsafe { WinDefWindowProc(h, msg, mp1, mp2) }
}

fn reset() {
    for p in &PAINTS {
        p.store(0, Ordering::Relaxed);
    }
    for r in &LOW_RECT {
        r.store(0, Ordering::Relaxed);
    }
}

fn paints(id: usize) -> u32 {
    PAINTS[id].load(Ordering::Relaxed)
}

pub fn paint_checks(hab: u32) {
    unsafe {
        WinRegisterClass(hab, b"PmxPaint\0".as_ptr(), paint_proc, 0, 0);
        // WS_VISIBLE | WS_CLIPSIBLINGS: three top-level windows, low,
        // mid (on low) and top (over low and part of mid).
        let mk = |id: u32, x: i32, y: i32, cx: i32, cy: i32| {
            WinCreateWindow(HWND_DESKTOP, b"PmxPaint\0".as_ptr(), b"\0".as_ptr(), 0x8000_0000 | 0x1000_0000, x, y, cx, cy, 0, 3, id, core::ptr::null(), core::ptr::null())
        };
        let low = mk(1, 400, 100, 300, 300);
        let mid = mk(2, 420, 120, 60, 60);
        let top = mk(3, 450, 150, 150, 150);
        crate::pump_for(hab, 300);
        reset();
        // mid moves away (out from under top): top has nothing to paint,
        // low only what mid covered.
        WinSetWindowPos(mid, 0, 650, 330, 0, 0, 0x0002);
        crate::pump_for(hab, 300);
        let lr: [i32; 4] = core::array::from_fn(|i| LOW_RECT[i].load(Ordering::Relaxed));
        check("a window above a moved one gets no WM_PAINT", paints(3) == 0);
        check(
            "the window it uncovered repaints only the uncovered part",
            paints(1) >= 1 && lr[0] >= 20 && lr[1] >= 20 && lr[2] <= 80 && lr[3] <= 80,
        );
        reset();
        // mid back, partly under top, then destroyed: top again has
        // nothing to paint.
        WinSetWindowPos(mid, 0, 430, 130, 0, 0, 0x0002);
        crate::pump_for(hab, 300);
        reset();
        WinDestroyWindow(mid);
        crate::pump_for(hab, 300);
        check("a window above a destroyed one gets no WM_PAINT", paints(3) == 0 && paints(1) >= 1);
        reset();
        // Hidden: the same (a new window, above top, beside it).
        let mid = mk(2, 410, 110, 30, 30);
        crate::pump_for(hab, 300);
        reset();
        WinShowWindow(mid, 0);
        crate::pump_for(hab, 300);
        check("a window above a hidden one gets no WM_PAINT", paints(3) == 0 && paints(1) >= 1);
        WinDestroyWindow(mid);
        WinDestroyWindow(top);
        WinDestroyWindow(low);
        crate::pump_for(hab, 200);
    }
}
