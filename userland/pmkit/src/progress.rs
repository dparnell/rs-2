//! A progress bar. PM has none (OS/2's own programs use a read-only
//! slider), so this is a window class of its own: a sunken trough filling
//! with blocks, the percentage beside it, and for work that cannot be
//! counted a run of blocks going to and fro.

use crate::*;

const CLASS: &[u8] = b"Rs2ProgressBar\0";
const BLOCK: i32 = 8;
const GAP: i32 = 2;
/// Blocks in the run that moves when nothing is counted.
const RUN: i32 = 5;
/// Room for "100%" on the right.
const LABEL: i32 = 44;

/// Window words: the percent, or the position of the run (with `SWEEPING`).
const QWL_POS: i32 = 0;
const SWEEPING: u32 = 0x8000_0000;

fn line(hps: u32, a: (i32, i32), b: (i32, i32), colour: i32) {
    unsafe {
        GpiSetColor(hps, colour);
        GpiMove(hps, &PointL { x: a.0, y: a.1 });
        GpiLine(hps, &PointL { x: b.0, y: b.1 });
    }
}

fn block(hps: u32, trough: &RectL, i: i32) {
    let l = trough.l + 2 + i * (BLOCK + GAP);
    let r = RectL { l, b: trough.b + 2, r: (l + BLOCK).min(trough.r - 2), t: trough.t - 2 };
    if r.r > r.l {
        unsafe { WinFillRect(hps, &r, SYSCLR_HILITEBACKGROUND) };
    }
}

extern "C" fn proc_(h: u32, msg: u32, mp1: u32, mp2: u32) -> u32 {
    if msg != WM_PAINT {
        return unsafe { WinDefWindowProc(h, msg, mp1, mp2) };
    }
    let mut dirty = RectL::default();
    let hps = unsafe { WinBeginPaint(h, 0, &mut dirty) };
    let mut all = RectL::default();
    unsafe {
        WinQueryWindowRect(h, &mut all);
        WinFillRect(hps, &all, SYSCLR_DIALOGBACKGROUND);
    }
    let pos = unsafe { WinQueryWindowULong(h, QWL_POS) };
    let sweeping = pos & SWEEPING != 0;
    let t = RectL { l: all.l, b: all.b + 2, r: all.r - LABEL, t: all.t - 2 };
    unsafe { WinFillRect(hps, &t, SYSCLR_WINDOW) };
    line(hps, (t.l, t.b), (t.l, t.t - 1), SYSCLR_BUTTONDARK);
    line(hps, (t.l, t.t - 1), (t.r - 1, t.t - 1), SYSCLR_BUTTONDARK);
    line(hps, (t.r - 1, t.t - 1), (t.r - 1, t.b), SYSCLR_BUTTONLIGHT);
    line(hps, (t.r - 1, t.b), (t.l, t.b), SYSCLR_BUTTONLIGHT);
    let blocks = (t.r - t.l - 4 + GAP) / (BLOCK + GAP);
    if sweeping {
        // To and fro over the trough.
        let span = (blocks - RUN).max(1);
        let n = (pos & !SWEEPING) as i32 % (span * 2);
        let first = if n < span { n } else { span * 2 - n };
        for i in first..first + RUN {
            block(hps, &t, i);
        }
    } else {
        for i in 0..blocks * pos.min(100) as i32 / 100 {
            block(hps, &t, i);
        }
        if pos > 0 {
            let text = z(&[&num(pos.min(100) as i64)[..], b"%"].concat());
            let mut r = RectL { l: t.r, b: all.b, r: all.r, t: all.t };
            unsafe { WinDrawText(hps, -1, text.as_ptr(), &mut r, SYSCLR_WINDOWTEXT, SYSCLR_DIALOGBACKGROUND, DT_RIGHT | DT_VCENTER) };
        }
    }
    unsafe { WinEndPaint(hps) };
    0
}

/// Make the window class known; once per program.
pub fn register(hab: u32) {
    unsafe { WinRegisterClass(hab, CLASS.as_ptr(), proc_, 0, 4) };
}

/// A progress bar in `parent`.
pub fn create(parent: u32, id: u32) -> u32 {
    unsafe { WinCreateWindow(parent, CLASS.as_ptr() as u32, b"\0".as_ptr(), WS_VISIBLE, 0, 0, 10, 10, parent, HWND_TOP, id, core::ptr::null(), core::ptr::null()) }
}

/// Show `percent` done (0: empty).
pub fn set(h: u32, percent: u32) {
    unsafe { WinSetWindowULong(h, QWL_POS, percent.min(100)) };
    invalidate(h);
}

/// One step of the run of blocks, for work that cannot be counted.
pub fn sweep(h: u32) {
    let pos = unsafe { WinQueryWindowULong(h, QWL_POS) };
    let next = if pos & SWEEPING != 0 { (pos & !SWEEPING) + 1 } else { 0 };
    unsafe { WinSetWindowULong(h, QWL_POS, next | SWEEPING) };
    invalidate(h);
}
