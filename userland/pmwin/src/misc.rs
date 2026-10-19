//! Initialisation, errors, system values and colours, string resources and
//! message boxes.

#![allow(non_snake_case)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use gpi::Surface;

use crate::color;
use crate::res;
use crate::state::{self, st};
use crate::win;

#[repr(C)]
#[derive(Default)]
struct DisplayInfo {
    address: u32,
    width: u32,
    height: u32,
    stride: u32,
    flags: u32,
}

unsafe extern "C" {
    #[link_name = "RS2API.1"]
    fn Rs2MapDisplay(info: *mut DisplayInfo) -> u32;
    #[link_name = "DOSCALLS.329"]
    fn DosWaitEventSem(hev: u32, ms: u32) -> u32;
    #[link_name = "DOSCALLS.334"]
    fn DosRequestMutexSem(hmtx: u32, ms: u32) -> u32;
    #[link_name = "DOSCALLS.286"]
    fn DosBeep(freq: u32, ms: u32) -> u32;
}

const HAB: u32 = 1;

/// This process has mapped the display.
static MAPPED: AtomicBool = AtomicBool::new(false);
/// WinGetLastError's value (per process).
pub static LAST_ERROR: AtomicU32 = AtomicU32::new(0);

/// WinInitialize(flOptions): the anchor block. The first call in a
/// process maps the display (a PM program joins the Presentation Manager
/// session); the first one creates the shared state.
#[unsafe(no_mangle)]
pub extern "C" fn WinInitialize(_fl: u32) -> u32 {
    if !MAPPED.load(Ordering::Relaxed) {
        let mut info = DisplayInfo::default();
        if unsafe { Rs2MapDisplay(&mut info) } != 0 {
            return 0;
        }
        // Built before anything shared points at it.
        gpi::font::system();
        MAPPED.store(true, Ordering::Relaxed);
        if state::get().is_none() {
            let surface = unsafe {
                Surface::new(info.address as *mut u32, info.width as i32, info.height as i32, info.stride as i32, info.flags & 1 != 0)
            };
            state::init(surface);
            win::register_builtin();
        }
    }
    HAB
}

#[unsafe(no_mangle)]
pub extern "C" fn WinTerminate(_hab: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryAnchorBlock(_h: u32) -> u32 {
    HAB
}

#[unsafe(no_mangle)]
pub extern "C" fn WinGetLastError(_hab: u32) -> u32 {
    LAST_ERROR.swap(0, Ordering::Relaxed)
}

/// No extended error information is kept.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetErrorInfo(_hab: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn WinFreeErrorInfo(_p: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinAlarm(_desk: u32, _style: u32) -> u32 {
    unsafe { DosBeep(880, 80) };
    1
}

/// WinQuerySysValue(hwndDesktop, iSysValue)
#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysValue(_desk: u32, i: i32) -> i32 {
    let s = st();
    match i {
        1 => 500,                    // SV_DBLCLKTIME
        2 | 3 => 4,                  // SV_CX/CYDBLCLK
        20 | 36 => s.surface.width,  // SV_CXSCREEN, SV_CXFULLSCREEN
        21 => s.surface.height,      // SV_CYSCREEN
        37 => s.surface.height - win::TITLE_H, // SV_CYFULLSCREEN
        4 | 5 => 4,                  // SV_CX/CYSIZEBORDER
        9 => 500,                    // SV_CURSORRATE
        10 => 300,                   // SV_FIRSTSCROLLRATE
        11 => 50,                    // SV_SCROLLRATE
        22 | 23 => 16,               // SV_CXVSCROLL / SV_CYHSCROLL
        24 | 25 => 16,               // SV_CYVSCROLLARROW / SV_CXHSCROLLARROW
        26 | 27 => 1,                // SV_CX/CYBORDER
        28 | 29 => 3,                // SV_CX/CYDLGFRAME
        30 => win::TITLE_H,          // SV_CYTITLEBAR
        31 | 32 => 16,               // SV_CYVSLIDER / SV_CXHSLIDER
        33 | 34 => 18,               // SV_CX/CYMINMAXBUTTON
        35 => 16,                    // SV_CYMENU
        38 | 39 => 32,               // SV_CX/CYICON
        40 | 41 => 32,               // SV_CX/CYPOINTER
        43 => 3,                     // SV_CMOUSEBUTTONS
        44 => -s.ptr_level.min(0),   // SV_POINTERLEVEL
        48 => 1,                     // SV_MOUSEPRESENT
        49 | 50 => 1,                // SV_CX/CYALIGN
        59 => 1,                     // SV_INSERTMODE
        66 => 1,                     // SV_ALTMNEMONIC
        64 | 65 => 0,                // SV_MENUROLLDOWN/UPDELAY
        68 => 75,                    // SV_CXICONTEXTWIDTH
        69 => 3,                     // SV_CICONTEXTLINES
        73 | 74 => 2,                // SV_CX/CYMOTIONSTART
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysColor(_desk: u32, clr: i32, _reserved: i32) -> u32 {
    color::sys(clr)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQuerySysPointer(_desk: u32, _id: i32, _copy: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetPointer(_desk: u32, _hptr: u32) -> u32 {
    1
}

/// CLASSINFO: flClassStyle, pfnWindowProc, cbWindowData.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryClassInfo(_hab: u32, name: *const u8, info: *mut u32) -> u32 {
    if name.is_null() || info.is_null() {
        return 0;
    }
    let v = name as u32;
    let n: Vec<u8> = if v >> 16 == 0xffff {
        alloc::format!("#{}", v & 0xffff).into_bytes()
    } else {
        (0..).map(|i| unsafe { *name.add(i) }).take_while(|&b| b != 0).collect()
    };
    let Some(c) = st().class(&n) else { return 0 };
    unsafe {
        info.write_unaligned(c.style);
        info.add(1).write_unaligned(c.proc_);
        info.add(2).write_unaligned(c.words);
    }
    1
}

/// WinSubclassWindow(hwnd, pfnwp): returns the previous procedure.
#[unsafe(no_mangle)]
pub extern "C" fn WinSubclassWindow(h: u32, proc_: u32) -> u32 {
    let Some(w) = st().desk.get_mut(h) else { return 0 };
    core::mem::replace(&mut w.proc_, proc_)
}

/// WinLoadString(hab, hmod, id, cchMax, pchBuffer)
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadString(_hab: u32, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    load_string(res::RT_STRING, hmod, id, cch, buf)
}

/// WinLoadMessage(hab, hmod, id, cchMax, pchBuffer): message tables have
/// the string table layout.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadMessage(_hab: u32, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    load_string(res::RT_MESSAGE, hmod, id, cch, buf)
}

fn load_string(ty: u32, hmod: u32, id: u32, cch: i32, buf: *mut u8) -> i32 {
    if buf.is_null() || cch <= 0 {
        return 0;
    }
    let text = res::string(hmod, ty, id).unwrap_or_default();
    crate::ctl::copy_out(&text, cch as usize, buf) as i32
}

/// WinWaitEventSem(hev, ulTimeout): waits without holding PMWIN.
#[unsafe(no_mangle)]
pub extern "C" fn WinWaitEventSem(hev: u32, ms: u32) -> u32 {
    crate::lock::unlocked(|| unsafe { DosWaitEventSem(hev, ms) })
}

/// WinRequestMutexSem(hmtx, ulTimeout)
#[unsafe(no_mangle)]
pub extern "C" fn WinRequestMutexSem(hmtx: u32, ms: u32) -> u32 {
    crate::lock::unlocked(|| unsafe { DosRequestMutexSem(hmtx, ms) })
}

/// Presentation parameters are accepted and ignored (controls use the
/// system colours and font).
#[unsafe(no_mangle)]
pub extern "C" fn WinSetPresParam(h: u32, _id: u32, _cb: u32, _pv: u32) -> u32 {
    st().desk.exists(h) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryPresParam(_h: u32, _id1: u32, _id2: u32, _pid: u32, _cb: u32, _pv: u32, _fs: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn WinRemovePresParam(h: u32, _id: u32) -> u32 {
    st().desk.exists(h) as u32
}

fn cstr(p: *const u8) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    (0..).map(|i| unsafe { *p.add(i) }).take_while(|&b| b != 0).collect()
}

/// WinMessageBox(hwndParent, hwndOwner, pszText, pszCaption, idWindow,
/// flStyle): a modal dialog with the text and the buttons of the style
/// (MB_OK .. MB_ENTERCANCEL); returns the MBID_* of the button pressed.
#[unsafe(no_mangle)]
pub extern "C" fn WinMessageBox(parent: u32, owner: u32, text: *const u8, title: *const u8, _id: u32, style: u32) -> u32 {
    const MBID_OK: u32 = 1;
    const MBID_CANCEL: u32 = 2;
    const MBID_ABORT: u32 = 3;
    const MBID_RETRY: u32 = 4;
    const MBID_IGNORE: u32 = 5;
    const MBID_YES: u32 = 6;
    const MBID_NO: u32 = 7;
    const MBID_ENTER: u32 = 9;
    let buttons: &[(u32, &[u8])] = match style & 0xf {
        1 => &[(MBID_OK, b"OK"), (MBID_CANCEL, b"Cancel")],
        2 => &[(MBID_RETRY, b"~Retry"), (MBID_CANCEL, b"Cancel")],
        3 => &[(MBID_ABORT, b"~Abort"), (MBID_RETRY, b"~Retry"), (MBID_IGNORE, b"~Ignore")],
        4 => &[(MBID_YES, b"~Yes"), (MBID_NO, b"~No")],
        5 => &[(MBID_YES, b"~Yes"), (MBID_NO, b"~No"), (MBID_CANCEL, b"Cancel")],
        6 => &[(MBID_CANCEL, b"Cancel")],
        7 => &[(MBID_ENTER, b"Enter")],
        8 => &[(MBID_ENTER, b"Enter"), (MBID_CANCEL, b"Cancel")],
        _ => &[(MBID_OK, b"OK")],
    };
    let font = gpi::font::system();
    let text = cstr(text);
    let title = cstr(title);
    let lines: Vec<Vec<u8>> = text.split(|&b| b == b'\n').map(|l| l.iter().copied().filter(|&b| b != b'\r').collect()).collect();
    let bw = 80;
    let tw = lines.iter().map(|l| font.text_width(l)).max().unwrap_or(0).max(font.text_width(&title) + 40).max(buttons.len() as i32 * (bw + 10)).max(160);
    // Template coordinates are dialog units (2x2 pixels).
    let (w, h) = (tw + 40, lines.len() as i32 * font.height + 60);
    let mut items = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let y = h - 10 - (i as i32 + 1) * font.height;
        // SS_TEXT | DT_LEFT, visible.
        items.push(res::DlgItemT { class: b"#5".to_vec(), text: l.clone(), style: 0x8000_0001, x: 5, y: y / 2, cx: (w - 20) / 2, cy: font.height / 2, id: 0xffff, ctl: core::ptr::null(), children: Vec::new() });
    }
    let total = buttons.len() as i32 * (bw + 10) - 10;
    for (i, (id, label)) in buttons.iter().enumerate() {
        // BS_PUSHBUTTON (the first one BS_DEFAULT), WS_TABSTOP, the first
        // one WS_GROUP, visible.
        let bs = 0x8000_0000 | 0x0002_0000 | if i == 0 { 0x0001_0400 } else { 0 };
        let x = (w - total) / 2 + i as i32 * (bw + 10);
        items.push(res::DlgItemT { class: b"#3".to_vec(), text: label.to_vec(), style: bs, x: x / 2, y: 4, cx: bw / 2, cy: 12, id: *id, ctl: core::ptr::null(), children: Vec::new() });
    }
    // FCF_TITLEBAR (control data); FS_DLGBORDER | FS_SCREENALIGN.
    let fcf: u32 = 0x0001;
    let (sw, sh) = (st().surface.width, st().surface.height);
    let t = res::DlgItemT {
        class: b"#1".to_vec(),
        text: title,
        style: 0x0080 | 0x0200,
        x: (sw - w) / 4,
        y: (sh - h) / 4,
        cx: w / 2,
        cy: h / 2,
        id: 0,
        ctl: &fcf as *const u32 as *const u8,
        children: items,
    };
    let parent = if parent == 0 { pmwin::HWND_DESKTOP } else { parent };
    let dlg = crate::dlg::create_from(parent, owner, 0, &t, 0);
    if dlg == 0 {
        return 0xffff;
    }
    let r = crate::dlg::WinProcessDlg(dlg);
    win::WinDestroyWindow(dlg);
    r
}

// ---- Version, code pages and strings ----

/// WinQueryVersion: PM 20.45 (OS/2 Warp 4.5x): major version in the low
/// byte, minor in the next.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryVersion(_hab: u32) -> u32 {
    20 | 45 << 8
}

#[unsafe(no_mangle)]
pub extern "C" fn WinQueryCp(_hmq: u32) -> u32 {
    850
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetCp(_hmq: u32, cp: u32) -> u32 {
    (cp == 850 || cp == 437) as u32
}

/// WinQueryCpList(hab, ccpMax, prgcp): the code pages PM can use.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryCpList(_hab: u32, max: u32, list: *mut u32) -> u32 {
    let cps = [850u32, 437];
    if !list.is_null() {
        for (i, cp) in cps.iter().take(max as usize).enumerate() {
            unsafe { list.add(i).write_unaligned(*cp) };
        }
    }
    cps.len() as u32
}

/// Upper case in code page 850.
pub fn upper850(c: u8) -> u8 {
    const PAIRS: [(u8, u8); 31] = [
        (0x87, 0x80), (0x81, 0x9a), (0x82, 0x90), (0x83, 0xb6), (0x84, 0x8e), (0x85, 0xb7), (0x86, 0x8f), (0x88, 0xd2),
        (0x89, 0xd3), (0x8a, 0xd4), (0x8b, 0xd8), (0x8c, 0xd7), (0x8d, 0xde), (0x91, 0x92), (0x93, 0xe2), (0x94, 0x99),
        (0x95, 0xe3), (0x96, 0xea), (0x97, 0xeb), (0x9b, 0x9d), (0xa0, 0xb5), (0xa1, 0xd6), (0xa2, 0xe0), (0xa3, 0xe9),
        (0xa4, 0xa5), (0xc6, 0xc7), (0xd0, 0xd1), (0xe4, 0xe5), (0xe7, 0xe8), (0xec, 0xed), (0x9c, 0x9c),
    ];
    if c.is_ascii_lowercase() {
        return c.to_ascii_uppercase();
    }
    PAIRS.iter().find(|p| p.0 == c).map_or(c, |p| p.1)
}

/// WinUpper(hab, idcp, idcc, psz): upper-cases the string in place;
/// returns its length.
#[unsafe(no_mangle)]
pub extern "C" fn WinUpper(_hab: u32, _cp: u32, _cc: u32, psz: *mut u8) -> u32 {
    if psz.is_null() {
        return 0;
    }
    let mut n = 0;
    loop {
        let p = unsafe { psz.add(n) };
        let c = unsafe { *p };
        if c == 0 {
            return n as u32;
        }
        unsafe { *p = upper850(c) };
        n += 1;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinUpperChar(_hab: u32, _cp: u32, _cc: u32, c: u32) -> u32 {
    if c > 0xff { c } else { upper850(c as u8) as u32 }
}

/// WinNextChar(hab, idcp, idcc, psz): single-byte code pages only.
#[unsafe(no_mangle)]
pub extern "C" fn WinNextChar(_hab: u32, _cp: u32, _cc: u32, psz: *const u8) -> u32 {
    if psz.is_null() || unsafe { *psz } == 0 { psz as u32 } else { psz as u32 + 1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn WinPrevChar(_hab: u32, _cp: u32, _cc: u32, start: *const u8, psz: *const u8) -> u32 {
    if psz as u32 <= start as u32 { start as u32 } else { psz as u32 - 1 }
}

/// WinCompareStrings(hab, idcp, idcc, psz1, psz2, reserved): WCS_EQ 1,
/// WCS_LT 2, WCS_GT 3 (case-insensitive).
#[unsafe(no_mangle)]
pub extern "C" fn WinCompareStrings(_hab: u32, _cp: u32, _cc: u32, a: *const u8, b: *const u8, _r: u32) -> u32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    let (a, b) = (cstr(a), cstr(b));
    let ka: Vec<u8> = a.iter().map(|&c| upper850(c)).collect();
    let kb: Vec<u8> = b.iter().map(|&c| upper850(c)).collect();
    match ka.cmp(&kb) {
        core::cmp::Ordering::Equal => 1,
        core::cmp::Ordering::Less => 2,
        core::cmp::Ordering::Greater => 3,
    }
}

// ---- Shutdown, window updates, owners ----

#[unsafe(no_mangle)]
pub extern "C" fn WinCancelShutdown(_hmq: u32, _always: u32) -> u32 {
    1
}

/// WinEnableWindowUpdate(hwnd, fEnable): drawing is not held back, so
/// turning updates on again only repaints the window.
#[unsafe(no_mangle)]
pub extern "C" fn WinEnableWindowUpdate(h: u32, on: u32) -> u32 {
    if !st().desk.exists(h) {
        return 0;
    }
    if on != 0 {
        st().desk.invalidate(h, None, true);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinLockWindowUpdate(_desk: u32, _h: u32) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinIsWindowShowing(h: u32) -> u32 {
    st().desk.is_showing(h) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn WinSetOwner(h: u32, owner: u32) -> u32 {
    match st().desk.get_mut(h) {
        Some(w) => {
            w.owner = owner;
            1
        }
        None => 0,
    }
}

/// WinQueryUpdateRect(hwnd, prcl): the bounds of the update region;
/// FALSE when nothing needs painting.
#[unsafe(no_mangle)]
pub extern "C" fn WinQueryUpdateRect(h: u32, prcl: *mut crate::gpiapi::RectL) -> u32 {
    let Some(w) = st().desk.get(h) else { return 0 };
    let b = w.update.bounds();
    if !prcl.is_null() {
        unsafe { prcl.write_unaligned(crate::gpiapi::RectL::from(b)) };
    }
    (!w.update.is_empty()) as u32
}

/// WinInvertRect(hps, prcl)
#[unsafe(no_mangle)]
pub extern "C" fn WinInvertRect(hps: u32, prcl: *const crate::gpiapi::RectL) -> u32 {
    if prcl.is_null() {
        return 0;
    }
    let r = unsafe { prcl.read_unaligned() }.rect();
    let s = st();
    crate::gpiapi::refresh(s, hps);
    let mut surface = s.surface;
    crate::pointer::hide(s);
    // DSTINVERT.
    let ok = crate::gpiapi::entry(s, hps).map(|e| e.ps.blit(&mut surface, r, None, 0x55, false, None)).is_some();
    crate::pointer::show(s);
    ok as u32
}

/// WinGetScreenPS(hwndDesktop): a presentation space on the whole screen.
#[unsafe(no_mangle)]
pub extern "C" fn WinGetScreenPS(_desk: u32) -> u32 {
    let s = st();
    let r = gpi::Rect::new(0, 0, s.surface.width, s.surface.height);
    crate::gpiapi::create(s, 0, gpi::Point::new(0, 0), alloc::vec![r])
}

/// Pointers from resources are not drawn yet (the arrow stays): a handle
/// that WinSetPointer accepts.
#[unsafe(no_mangle)]
pub extern "C" fn WinLoadPointer(_desk: u32, _hmod: u32, id: u32) -> u32 {
    0x7000_0000 | (id & 0xffff)
}

#[unsafe(no_mangle)]
pub extern "C" fn WinDestroyPointer(_hptr: u32) -> u32 {
    1
}

/// WinTrackRect(hwnd, hps, ptrackinfo): interactive rectangle tracking
/// is not available; reported as cancelled.
#[unsafe(no_mangle)]
pub extern "C" fn WinTrackRect(_h: u32, _hps: u32, _ti: u32) -> u32 {
    0
}

// ---- Hooks ----

/// WinSetHook(hab, hmq, iHook, pfnHook, hmod): hooks are recorded (the
/// help hook, HK_HELP, is how programs see help requests; the help
/// manager does not show help yet, so it is never called).
#[unsafe(no_mangle)]
pub extern "C" fn WinSetHook(_hab: u32, hmq: u32, kind: u32, pfn: u32, hmod: u32) -> u32 {
    if pfn == 0 {
        return 0;
    }
    let s = st();
    // HMQ_CURRENT (1) is the caller's queue.
    let queue = if hmq == 1 { s.my_queue() } else { hmq };
    let me = state::pid();
    s.hooks.insert(0, state::Hook { queue, kind, pfn, hmod, pid: me });
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn WinReleaseHook(_hab: u32, hmq: u32, kind: u32, pfn: u32, _hmod: u32) -> u32 {
    let s = st();
    let queue = if hmq == 1 { s.my_queue() } else { hmq };
    let me = state::pid();
    let n = s.hooks.len();
    if let Some(i) = s.hooks.iter().position(|h| h.queue == queue && h.kind == kind && h.pfn == pfn && h.pid == me) {
        s.hooks.remove(i);
    }
    (s.hooks.len() != n) as u32
}
