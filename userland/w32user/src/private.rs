//! W32USER's private calls for WIN16, its 16-bit facade (kern.rs has the
//! host interface): what WIN16's own parts (DDE, the clipboard, TOOLHELP,
//! Windows Help) need of the window manager beyond Win32's calls.

use crate::wnd::{self, hw, wnd};
use crate::wr32;

/// Rs2W32Queue(phab, phmq): the thread's PM queue (made now if it has
/// none): 1, or 0 if PM refused one.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32Queue(phab: u32, phmq: u32) -> u32 {
    let q = crate::queue();
    if phab != 0 {
        wr32(phab, 0, q.hab);
    }
    if phmq != 0 {
        wr32(phmq, 0, q.hmq);
    }
    (q.hmq != 0) as u32
}

/// Rs2W32PmWindow(hwnd, frame): a window's PM client window (its frame
/// if `frame`), 0 for none.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32PmWindow(h: u32, frame: u32) -> u32 {
    wnd(hw(h)).map_or(0, |w| if frame != 0 { w.frame } else { w.client })
}

/// Rs2W32HwndOfPm(pm, above): the window of a PM window (its client or
/// frame); with `above`, of the nearest PM window up its parents that is
/// one.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32HwndOfPm(pm: u32, above: u32) -> u32 {
    (if above != 0 { wnd::hwnd_above(pm) } else { wnd::hwnd_of(pm) }) as u32
}

/// Rs2W32Builtin(hwnd): which built-in class a window's class procedure
/// is (subclassed windows and superclasses count): 1 BUTTON, 2 STATIC,
/// 3 EDIT, 4 LISTBOX, 5 COMBOBOX, 6 SCROLLBAR, 7 MDICLIENT, 8 the dialog
/// class; 0 for others.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32Builtin(h: u32) -> u32 {
    let Some(ci) = wnd(hw(h)).map(|w| w.class) else { return 0 };
    let Some(p) = wnd::class(ci).map(|c| c.proc_) else { return 0 };
    let Some(f) = wnd::native(p) else { return 0 };
    let is = |g: wnd::Native| f as usize == g as usize;
    if is(crate::ctl::button) {
        1
    } else if is(crate::ctl::static_) {
        2
    } else if is(crate::edit::proc_) {
        3
    } else if is(crate::listbox::proc_) {
        4
    } else if is(crate::listbox::combo_proc) {
        5
    } else if is(crate::scroll::control_proc) {
        6
    } else if is(crate::mdi::client_proc) {
        7
    } else if is(crate::dialog::def_dlg_proc) {
        8
    } else {
        0
    }
}

/// Rs2W32Windows(buf, max, top): the windows (top-level ones only if
/// `top`), oldest first: how many there are (up to `max` are stored).
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32Windows(buf: u32, max: u32, top: u32) -> u32 {
    let v = if top != 0 { wnd::top_levels() } else { wnd::all() };
    for (i, h) in v.iter().take(max as usize).enumerate() {
        if buf != 0 {
            wr32(buf, 4 * i as u32, *h as u32);
        }
    }
    v.len() as u32
}

/// Rs2W32ClassInfo(i, name, max, phinst): the i-th class's name (upper
/// case) and module: 1, or 0 past the last.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32ClassInfo(i: u32, name: u32, max: u32, phinst: u32) -> u32 {
    let Some(c) = wnd::class(i as usize) else { return 0 };
    crate::copy_out(&c.name, name as *mut u8, max as i32);
    if phinst != 0 {
        wr32(phinst, 0, c.hinst);
    }
    1
}

/// Rs2W32PmBitmap(hbm): a bitmap's PM bitmap (0 for none).
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32PmBitmap(h: u32) -> u32 {
    crate::bitmap::bitmap_of(h as u16).map_or(0, |b| b.0)
}

/// Rs2W32BitmapFromDib(dib, n): a bitmap made from a packed DIB
/// (BITMAPINFO, then the bits).
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32BitmapFromDib(p: u32, n: u32) -> u32 {
    if p == 0 {
        return 0;
    }
    crate::bitmap::from_dib(unsafe { core::slice::from_raw_parts(p as *const u8, n as usize) }) as u32
}

/// Rs2W32Destroying(hwnd): DestroyWindow is under way for it.
#[unsafe(no_mangle)]
pub extern "stdcall" fn Rs2W32Destroying(h: u32) -> u32 {
    wnd(hw(h)).is_some_and(|w| w.destroying) as u32
}
