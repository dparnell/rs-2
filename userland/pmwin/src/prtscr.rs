//! The Print Screen key: PM prints a window to the default printer, as
//! IBM's PMWIN does (winprint.c in PMMERGE: CheckPrintScreen,
//! WinPrintScreen, its print thread).
//!
//! - PrtSc pressed (VK_PRINTSCRN without Ctrl or Alt) while
//!   SV_PRINTSCREEN is on (the System object's Print Screen page) asks
//!   the server's desktop window to print (input.rs, server.rs); a print
//!   still running makes a beep instead.
//! - What is printed: the top-level window under the mouse pointer, or
//!   the active window with `SET PM_PRINTSCREEN_ACTIVE_WINDOW=ON` (or
//!   without a mouse); over the desktop, the whole screen.
//! - A thread of the server copies the window's part of the screen into
//!   a bitmap and prints it on a PM_Q_STD queued DC of the default
//!   printer (`PM_SPOOLER` `QUEUE` in OS2SYS.INI): the picture as large as
//!   a whole multiple of its size that fits the page with a margin (or
//!   shrunk to fit), centred, as IBM's does. The document is "Print
//!   Screen".

use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::gpiapi::{PointL, RectL};

unsafe extern "C" {
    #[link_name = "DOSCALLS.286"]
    fn DosBeep(freq: u32, ms: u32) -> u32;
}

/// Call a PM function through its export entry point (which takes the
/// PM lock, as for any program).
macro_rules! pm {
    ($name:ident, fn($($t:ty),*) -> $r:ty, $($a:expr),*) => {{
        let f: unsafe extern "C" fn($($t),*) -> $r = unsafe { core::mem::transmute(crate::exports::$name as *const ()) };
        unsafe { f($($a),*) }
    }};
}

/// A print running (IBM's fProcessed).
static BUSY: AtomicBool = AtomicBool::new(false);

const OD_QUEUED: i32 = 2;
const OD_MEMORY: i32 = 8;
const PU_PELS: u32 = 0x0008;
const GPIA_ASSOC: u32 = 0x4000;
const ROP_SRCCOPY: i32 = 0xcc;
const BBO_IGNORE: u32 = 2;
const DEVESC_STARTDOC: i32 = 8150;
const DEVESC_ENDDOC: i32 = 8151;
const CAPS_WIDTH: i32 = 4;
const QW_PARENT: i32 = 5;
const HWND_DESKTOP: u32 = 1;

/// WM_RS2_PRINTSCREEN arrived at the server's desktop window: start the
/// print thread (one at a time).
pub fn start() {
    if BUSY.swap(true, Ordering::AcqRel) {
        unsafe { DosBeep(2000, 500) };
        return;
    }
    let mut tid = 0;
    if unsafe { os2::DosCreateThread(&mut tid, thread, 0, 0, 1024 * 1024) } != 0 {
        BUSY.store(false, Ordering::Release);
    }
}

/// Whether an environment variable of this process is `ON`.
fn env_on(name: &[u8]) -> bool {
    let n = [name, b"\0"].concat();
    let mut p: *const u8 = null();
    if unsafe { os2::DosScanEnv(n.as_ptr(), &mut p) } != 0 || p.is_null() {
        return false;
    }
    let mut v = Vec::new();
    unsafe {
        let mut q = p;
        while *q != 0 {
            v.push(*q);
            q = q.add(1);
        }
    }
    v.eq_ignore_ascii_case(b"ON")
}

/// The window to print: the top-level window under the pointer (or the
/// active one); the desktop for the whole screen.
fn target() -> u32 {
    let mouse = pm!(__a_WinQuerySysValue, fn(u32, i32) -> i32, HWND_DESKTOP, 48) != 0; // SV_MOUSEPRESENT
    let mut h = if !mouse || env_on(b"PM_PRINTSCREEN_ACTIVE_WINDOW") {
        pm!(__a_WinQueryActiveWindow, fn(u32) -> u32, HWND_DESKTOP)
    } else {
        let mut p = PointL { x: 0, y: 0 };
        pm!(__a_WinQueryPointerPos, fn(u32, *mut PointL) -> u32, HWND_DESKTOP, &mut p);
        pm!(__a_WinWindowFromPoint, fn(u32, *const PointL, u32) -> u32, HWND_DESKTOP, &p, 0)
    };
    if h == 0 {
        return HWND_DESKTOP;
    }
    // Up to the child of the desktop.
    loop {
        let parent = pm!(__a_WinQueryWindow, fn(u32, i32) -> u32, h, QW_PARENT);
        if parent == 0 || parent == HWND_DESKTOP {
            return h;
        }
        h = parent;
    }
}

/// The default printer's queue (`PM_SPOOLER` `QUEUE`: `NAME;`).
fn default_queue() -> Option<Vec<u8>> {
    let mut hmod = 0;
    let mut fail = [0u8; 16];
    if unsafe { os2::DosLoadModule(fail.as_mut_ptr(), 16, b"PMSHAPI\0".as_ptr(), &mut hmod) } != 0 {
        return None;
    }
    let mut f = 0usize;
    // PrfQueryProfileString
    if unsafe { os2::DosQueryProcAddr(hmod, 115, null(), &mut f) } != 0 || f == 0 {
        return None;
    }
    let query: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, *mut u8, u32) -> u32 = unsafe { core::mem::transmute(f) };
    let mut buf = [0u8; 64];
    let n = unsafe { query(0xffff_fffe, b"PM_SPOOLER\0".as_ptr(), b"QUEUE\0".as_ptr(), b"\0".as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let q: Vec<u8> = buf[..n.min(buf.len())].iter().copied().take_while(|&c| c != 0 && c != b';').collect();
    (!q.is_empty()).then_some(q)
}

extern "C" fn thread(_: u32) {
    let hab = pm!(__a_WinInitialize, fn(u32) -> u32, 0);
    let hmq = pm!(__a_WinCreateMsgQueue, fn(u32, i32) -> u32, hab, 0);
    pm!(__a_WinCancelShutdown, fn(u32, u32) -> u32, hmq, 1);
    let h = target();
    if !print_window(hab, h) {
        pm!(__a_WinAlarm, fn(u32, u32) -> u32, HWND_DESKTOP, 2); // WA_ERROR
    }
    pm!(__a_WinDestroyMsgQueue, fn(u32) -> u32, hmq);
    BUSY.store(false, Ordering::Release);
}

/// Print window `h` (its part of the screen) on the default printer.
fn print_window(hab: u32, h: u32) -> bool {
    // The window's rectangle on the screen, clipped to it.
    let mut r = RectL::default();
    pm!(__a_WinQueryWindowRect, fn(u32, *mut RectL) -> u32, h, &mut r);
    let mut pts = [PointL { x: r.left, y: r.bottom }, PointL { x: r.right, y: r.top }];
    pm!(__a_WinMapWindowPoints, fn(u32, u32, *mut PointL, i32) -> u32, h, HWND_DESKTOP, pts.as_mut_ptr(), 2);
    let sw = pm!(__a_WinQuerySysValue, fn(u32, i32) -> i32, HWND_DESKTOP, 20);
    let sh = pm!(__a_WinQuerySysValue, fn(u32, i32) -> i32, HWND_DESKTOP, 21);
    let (x0, y0) = (pts[0].x.max(0), pts[0].y.max(0));
    let (x1, y1) = (pts[1].x.min(sw), pts[1].y.min(sh));
    let (w, ht) = (x1 - x0, y1 - y0);
    if w <= 0 || ht <= 0 {
        return false;
    }
    // A memory bitmap with the screen's pels.
    let mdc = pm!(__a_DevOpenDC, fn(u32, i32, *const u8, i32, *const u32, u32) -> u32, hab, OD_MEMORY, b"*\0".as_ptr(), 0, null(), 0);
    let size = [0i32, 0];
    let mps = pm!(__a_GpiCreatePS, fn(u32, u32, *const i32, u32) -> u32, hab, mdc, size.as_ptr(), PU_PELS | GPIA_ASSOC);
    // BITMAPINFOHEADER2: cbFix, cx, cy, cPlanes, cBitCount.
    let mut hdr = [0u8; 16];
    hdr[0..4].copy_from_slice(&16u32.to_le_bytes());
    hdr[4..8].copy_from_slice(&(w as u32).to_le_bytes());
    hdr[8..12].copy_from_slice(&(ht as u32).to_le_bytes());
    hdr[12..14].copy_from_slice(&1u16.to_le_bytes());
    hdr[14..16].copy_from_slice(&24u16.to_le_bytes());
    let hbm = pm!(__a_GpiCreateBitmap, fn(u32, *const u8, u32, *const u8, *const u8) -> u32, mps, hdr.as_ptr(), 0, null(), null());
    if hbm == 0 {
        pm!(__a_GpiDestroyPS, fn(u32) -> u32, mps);
        pm!(__a_DevCloseDC, fn(u32) -> u32, mdc);
        return false;
    }
    pm!(__a_GpiSetBitmap, fn(u32, u32) -> u32, mps, hbm);
    // The desktop's PS reads the screen (its pels, whatever covers them).
    let screen = pm!(__a_WinGetPS, fn(u32) -> u32, HWND_DESKTOP);
    let blt = [PointL { x: 0, y: 0 }, PointL { x: w, y: ht }, PointL { x: x0, y: y0 }];
    pm!(__a_GpiBitBlt, fn(u32, u32, i32, *const PointL, i32, u32) -> i32, mps, screen, 3, blt.as_ptr(), ROP_SRCCOPY, BBO_IGNORE);
    pm!(__a_WinReleasePS, fn(u32) -> u32, screen);
    pm!(__a_GpiSetBitmap, fn(u32, u32) -> u32, mps, 0);
    let ok = print_bitmap(hab, hbm, w, ht);
    pm!(__a_GpiDeleteBitmap, fn(u32) -> u32, hbm);
    pm!(__a_GpiAssociate, fn(u32, u32) -> u32, mps, 0);
    pm!(__a_GpiDestroyPS, fn(u32) -> u32, mps);
    pm!(__a_DevCloseDC, fn(u32) -> u32, mdc);
    ok
}

/// The bitmap (w x h pels) on a page of the default printer.
fn print_bitmap(hab: u32, hbm: u32, w: i32, h: i32) -> bool {
    let Some(queue) = default_queue() else { return false };
    let q = [&queue[..], b"\0"].concat();
    let dop: [u32; 9] = [q.as_ptr() as u32, 0, 0, b"PM_Q_STD\0".as_ptr() as u32, b"Print Screen\0".as_ptr() as u32, 0, b"XFM=0\0".as_ptr() as u32, 0, 0];
    let hdc = pm!(__a_DevOpenDC, fn(u32, i32, *const u8, i32, *const u32, u32) -> u32, hab, OD_QUEUED, b"*\0".as_ptr(), 9, dop.as_ptr(), 0);
    if hdc == 0 {
        return false;
    }
    let mut page = [0i32; 2];
    pm!(__a_DevQueryCaps, fn(u32, i32, i32, *mut i32) -> u32, hdc, CAPS_WIDTH, 2, page.as_mut_ptr());
    let size = [page[0], page[1]];
    let ps = pm!(__a_GpiCreatePS, fn(u32, u32, *const i32, u32) -> u32, hab, hdc, size.as_ptr(), PU_PELS | GPIA_ASSOC);
    // A whole multiple of the picture that fits inside a margin of 1/39
    // of the page, else the picture shrunk to fit; centred.
    let (pw, ph) = (page[0] - page[0] / 39, page[1] - page[1] / 39);
    let n = (pw / w).min(ph / h);
    let (dw, dh) = if n >= 1 {
        (w * n, h * n)
    } else if w * ph > h * pw {
        (pw, h * pw / w)
    } else {
        (w * ph / h, ph)
    };
    let (dx, dy) = ((page[0] - dw) / 2, (page[1] - dh) / 2);
    let doc = b"Print Screen\0";
    let esc = |code: i32, n: i32, p: *const u8, nout: *mut i32, out: *mut u8| pm!(__a_DevEscape, fn(u32, i32, i32, *const u8, *mut i32, *mut u8) -> i32, hdc, code, n, p, nout, out);
    let mut ok = esc(DEVESC_STARTDOC, doc.len() as i32 - 1, doc.as_ptr(), null_mut(), null_mut()) != -1;
    if ok {
        let pts = [PointL { x: dx, y: dy }, PointL { x: dx + dw, y: dy + dh }, PointL { x: 0, y: 0 }, PointL { x: w, y: h }];
        ok = pm!(__a_GpiWCBitBlt, fn(u32, u32, i32, *const PointL, i32, u32) -> i32, ps, hbm, 4, pts.as_ptr(), ROP_SRCCOPY, BBO_IGNORE) != 0;
        let mut id = [0u8; 2];
        let mut nid = 2i32;
        ok &= esc(DEVESC_ENDDOC, 0, null(), &mut nid, id.as_mut_ptr()) != -1;
    }
    pm!(__a_GpiAssociate, fn(u32, u32) -> u32, ps, 0);
    pm!(__a_GpiDestroyPS, fn(u32) -> u32, ps);
    pm!(__a_DevCloseDC, fn(u32) -> u32, hdc);
    ok
}
